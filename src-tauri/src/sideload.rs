//! Port of backend/src/utils/sideload.ts -- SideForge's recursive sideload
//! pipeline: single APK/OBB, directory of mixed APK+OBB pairs, optional
//! install.txt script, auto-reinstall-with-data-backup on signature
//! mismatch/downgrade/storage errors.
//!
//! Progress is emitted as Tauri events named "adb_event", in the same shape
//! the Node version's `io.emit('adb_event', ...)` used, so an eventual
//! frontend port of the existing UI needs minimal changes.

use crate::adb;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Serialize, Clone)]
pub struct InstallLogEntry {
    pub apk: String,
    pub success: bool,
    pub reinstalled: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct ObbLogEntry {
    pub pkg: String,
    pub success: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct SideloadResult {
    pub success: bool,
    pub message: String,
    #[serde(rename = "installLog")]
    pub install_log: Vec<InstallLogEntry>,
    #[serde(rename = "obbLog")]
    pub obb_log: Vec<ObbLogEntry>,
}

#[derive(Debug, Serialize, Clone)]
struct AdbEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    message: String,
    #[serde(rename = "folderPath")]
    folder_path: String,
    #[serde(rename = "currentName")]
    current_name: Option<String>,
    total: usize,
    current: usize,
    step: u8,
    #[serde(rename = "assetType")]
    asset_type: &'static str,
}

fn emit_progress(
    app: &AppHandle,
    folder_path: &str,
    msg: &str,
    step: u8,
    current_name: Option<&str>,
    total: usize,
    current: usize,
    asset_type: &'static str,
) {
    let _ = app.emit(
        "adb_event",
        AdbEvent {
            kind: "progress",
            message: msg.to_string(),
            folder_path: folder_path.to_string(),
            current_name: current_name.map(|s| s.to_string()),
            total,
            current,
            step,
            asset_type,
        },
    );
}

/// Translated progress message text -- a direct port of sideload.ts's small
/// `getMsg()` table (just the "en"/"pt" pairs the original shipped).
fn msg(lang: &str, key: &str, data: &str) -> String {
    let pt = lang == "pt";
    match key {
        "installing_apk" => if pt { format!("Instalando APK: {data}") } else { format!("Installing APK: {data}") },
        "pushing_companion" => if pt { format!("Enviando OBB companheira: {data}") } else { format!("Pushing companion OBB: {data}") },
        "pushing_lone_obb" => if pt { format!("Enviando OBB avulso: {data}") } else { format!("Pushing lone OBB: {data}") },
        "pushing_obb" => if pt { format!("Enviando OBB: {data}") } else { format!("Pushing OBB: {data}") },
        "pushing_loose_obb" => if pt { format!("Enviando OBB solto: {data}") } else { format!("Pushing loose OBB: {data}") },
        "reinstalling" => if pt {
            "Reinstalação (Backup -> Desinstalar -> Instalar -> Restaurar)...".to_string()
        } else {
            "Reinstalling (Backup -> Uninstall -> Install -> Restore)...".to_string()
        },
        "backing_up" => if pt { "Fazendo backup dos dados salvos...".to_string() } else { "Backing up save data...".to_string() },
        "uninstalling" => if pt { "Desinstalando versão antiga...".to_string() } else { "Uninstalling old version...".to_string() },
        "installing_new" => if pt { "Instalando nova versão...".to_string() } else { "Installing new version...".to_string() },
        "restoring" => if pt { "Restaurando dados salvos...".to_string() } else { "Restoring save data...".to_string() },
        _ => key.to_string(),
    }
}

fn msg_custom_command(lang: &str, current: usize, total: usize, cmd: &str) -> String {
    if lang == "pt" {
        format!("Executando comando {current}/{total}: {cmd}")
    } else {
        format!("Running command {current}/{total}: {cmd}")
    }
}

/// Port of `performSideForgeSideload()`.
pub async fn perform_sideforge_sideload(
    app: &AppHandle,
    target_path: &str,
    device_id: Option<&str>,
    lang: &str,
) -> SideloadResult {
    let absolute = std::fs::canonicalize(target_path).unwrap_or_else(|_| PathBuf::from(target_path));
    let absolute_str = absolute.to_string_lossy().to_string();
    let mut install_log = Vec::new();
    let mut obb_log = Vec::new();

    let metadata = match fs::metadata(&absolute) {
        Ok(m) => m,
        Err(e) => return SideloadResult { success: false, message: e.to_string(), install_log, obb_log },
    };

    if metadata.is_file() {
        let filename = absolute.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

        if filename.ends_with(".apk") {
            let pkg = adb::get_apk_package_name(&absolute_str)
                .await
                .unwrap_or_else(|| filename.trim_end_matches(".apk").to_string());
            emit_progress(app, &absolute_str, &msg(lang, "installing_apk", &filename), 1, Some(&filename), 1, 0, "apk");

            let (success, reinstalled) = sideload_apk_with_retry(&absolute_str, &pkg, device_id, lang, |m, step| {
                emit_progress(app, &absolute_str, m, step, Some(&filename), 1, 0, "apk");
            })
            .await;
            install_log.push(InstallLogEntry { apk: filename.clone(), success, reinstalled });

            if let Some(parent) = absolute.parent() {
                let potential_obb_dir = parent.join(&pkg);
                if potential_obb_dir.is_dir() {
                    emit_progress(app, &absolute_str, &msg(lang, "pushing_companion", &pkg), 1, Some(&pkg), 1, 0, "obb");
                    let ok = push_obb_safe(&potential_obb_dir.to_string_lossy(), &pkg, device_id, |_m, _s| {}).await;
                    obb_log.push(ObbLogEntry { pkg: pkg.clone(), success: ok });
                }
            }
        } else if filename.ends_with(".obb") {
            let pkg = extract_obb_pkg(&filename).unwrap_or_else(|| filename.trim_end_matches(".obb").to_string());
            emit_progress(app, &absolute_str, &msg(lang, "pushing_lone_obb", &pkg), 1, Some(&pkg), 1, 0, "obb");

            let temp_dir = absolute.parent().unwrap_or(Path::new(".")).join(format!("_temp_obb_{pkg}"));
            let _ = fs::create_dir_all(&temp_dir);
            let dest = temp_dir.join(&filename);
            let _ = fs::copy(&absolute, &dest);

            let ok = push_obb_safe(&temp_dir.to_string_lossy(), &pkg, device_id, |_m, _s| {}).await;
            obb_log.push(ObbLogEntry { pkg, success: ok });

            let _ = fs::remove_file(&dest);
            let _ = fs::remove_dir(&temp_dir);
        } else if filename == "install.txt" || filename.ends_with(".txt") {
            let success = run_install_txt(&absolute_str, device_id, lang, |m, step| {
                emit_progress(app, &absolute_str, m, step, None, 1, 0, "apk");
            })
            .await;
            install_log.push(InstallLogEntry { apk: "install.txt".to_string(), success, reinstalled: false });
        }
    } else {
        let install_txt = absolute.join("install.txt");
        if install_txt.exists() {
            run_install_txt(&install_txt.to_string_lossy(), device_id, lang, |m, step| {
                emit_progress(app, &absolute_str, m, step, None, 1, 0, "apk");
            })
            .await;
        }

        let (apks, mut obbs) = find_sideload_assets(&absolute);
        let total = apks.len() + obbs.len();
        let mut current_idx = 0usize;

        for apk in &apks {
            let pkg = adb::get_apk_package_name(apk).await.unwrap_or_else(|| {
                Path::new(apk).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
            });
            let filename = Path::new(apk).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            emit_progress(app, &absolute_str, &msg(lang, "installing_apk", &filename), 1, Some(&filename), total, current_idx, "apk");

            let (success, reinstalled) = sideload_apk_with_retry(apk, &pkg, device_id, lang, |m, step| {
                emit_progress(app, &absolute_str, m, step, Some(&filename), total, current_idx, "apk");
            })
            .await;
            install_log.push(InstallLogEntry { apk: filename.clone(), success, reinstalled });
            current_idx += 1;

            if let Some(pos) = obbs
                .iter()
                .position(|o| Path::new(o).file_name().map(|n| n.to_string_lossy().to_string()).as_deref() == Some(pkg.as_str()))
            {
                let matching_obb = obbs.remove(pos);
                emit_progress(app, &absolute_str, &msg(lang, "pushing_obb", &pkg), 1, Some(&pkg), total, current_idx, "obb");
                let ok = push_obb_safe(&matching_obb, &pkg, device_id, |_m, _s| {}).await;
                obb_log.push(ObbLogEntry { pkg: pkg.clone(), success: ok });
                current_idx += 1;
            }
        }

        for obb in &obbs {
            let pkg = Path::new(obb).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            emit_progress(app, &absolute_str, &msg(lang, "pushing_loose_obb", &pkg), 1, Some(&pkg), total, current_idx, "obb");
            let ok = push_obb_safe(obb, &pkg, device_id, |_m, _s| {}).await;
            obb_log.push(ObbLogEntry { pkg, success: ok });
            current_idx += 1;
        }
    }

    SideloadResult { success: true, message: "Sideload completed".to_string(), install_log, obb_log }
}

/// Port of `sideloadApkWithRetry()`: installs once; on a signature
/// mismatch, version downgrade, generic install failure, or low storage,
/// backs up the package's data, uninstalls, reinstalls, and restores the
/// backup on success.
async fn sideload_apk_with_retry(
    apk_path: &str,
    pkg: &str,
    device_id: Option<&str>,
    lang: &str,
    mut on_progress: impl FnMut(&str, u8),
) -> (bool, bool) {
    let mut result = adb::install_app(apk_path, device_id, |step| on_progress("", step)).await;
    let mut reinstalled = false;

    if !result.success {
        let combined = format!("{}{}", result.stderr.to_lowercase(), result.stdout.to_lowercase());
        let reinstall_eligible = combined.contains("signatures do not match")
            || combined.contains("install_failed_version_downgrade")
            || combined.contains("failed to install")
            || combined.contains("insufficient_storage");

        if reinstall_eligible {
            on_progress(&msg(lang, "reinstalling", ""), 1);

            let backup_path = Path::new(apk_path)
                .parent()
                .map(|p| p.join(format!("_backup_{pkg}")))
                .unwrap_or_else(|| PathBuf::from(format!("_backup_{pkg}")));
            let backup_path_str = backup_path.to_string_lossy().to_string();

            on_progress(&msg(lang, "backing_up", ""), 1);
            let _ = adb::pull_path(&format!("/sdcard/Android/data/{pkg}"), &backup_path_str, device_id).await;

            on_progress(&msg(lang, "uninstalling", ""), 1);
            adb::uninstall_app(pkg, device_id).await;

            on_progress(&msg(lang, "installing_new", ""), 2);
            let second_try = adb::install_app(apk_path, device_id, |step| on_progress("", step)).await;

            if second_try.success {
                reinstalled = true;
                if backup_path.exists() {
                    on_progress(&msg(lang, "restoring", ""), 2);
                    let _ = adb::push_path(&backup_path_str, "/sdcard/Android/data/", device_id).await;
                    let _ = fs::remove_dir_all(&backup_path);
                }
            }
            result = second_try;
        }
    }

    (result.success, reinstalled)
}

async fn push_obb_safe(obb_dir: &str, pkg: &str, device_id: Option<&str>, mut on_progress: impl FnMut(&str, u8)) -> bool {
    adb::push_obb(obb_dir, pkg, device_id, |step| on_progress("", step)).await.is_ok()
}

/// Port of `findSideloadAssets()`: recursively walks `dir`; a subfolder
/// containing any `.obb` file is treated as an OBB bundle (not recursed
/// into further), everything else is recursed, and loose `.apk` files are
/// collected individually.
fn find_sideload_assets(dir: &Path) -> (Vec<String>, Vec<String>) {
    let mut apks = Vec::new();
    let mut obbs = Vec::new();
    recurse_assets(dir, &mut apks, &mut obbs);
    (apks, obbs)
}

fn recurse_assets(dir: &Path, apks: &mut Vec<String>, obbs: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let has_obb = fs::read_dir(&path)
                .map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".obb")))
                .unwrap_or(false);
            if has_obb {
                obbs.push(path.to_string_lossy().to_string());
            } else {
                recurse_assets(&path, apks, obbs);
            }
        } else if path.extension().map(|e| e == "apk").unwrap_or(false) {
            apks.push(path.to_string_lossy().to_string());
        }
    }
}

/// Port of `runInstallTxt()`: runs each non-blank, non-`#`-comment line as
/// an adb subcommand. See `adb::run_adb_command`'s doc note -- lines with
/// quoted spaces in an argument aren't parsed the way the Node version's
/// shell-based exec would have handled them.
async fn run_install_txt(file_path: &str, device_id: Option<&str>, lang: &str, mut on_progress: impl FnMut(&str, u8)) -> bool {
    let Ok(content) = fs::read_to_string(file_path) else { return false };
    let lines: Vec<&str> = content.lines().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let total = lines.len();
    if total == 0 {
        return true;
    }
    for (i, cmd) in lines.iter().enumerate() {
        let step = (((i as f64 / total as f64) * 3.0).floor() as u8 + 1).clamp(1, 3);
        on_progress(&msg_custom_command(lang, i + 1, total, cmd), step);
        let _ = adb::run_adb_command(cmd, device_id).await;
    }
    true
}

/// "main.123.com.pkg.name.obb" or "patch.123.com.pkg.name.obb" -> "com.pkg.name".
fn extract_obb_pkg(filename: &str) -> Option<String> {
    let stripped = filename.strip_suffix(".obb")?;
    let rest = stripped.strip_prefix("main.").or_else(|| stripped.strip_prefix("patch."))?;
    let dot = rest.find('.')?;
    Some(rest[dot + 1..].to_string())
}
