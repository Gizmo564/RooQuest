//! Port of backend/src/utils/adb.ts -- ADB command resolution, device
//! listing/info, install/uninstall, and the push+install+cleanup pipeline
//! used by sideload.rs.

use std::path::PathBuf;
use tokio::process::Command;

/// Looks for the bundled adb binary under the app's CURRENT data-dir name
/// (RooQuest) first, then falls back to checking its older names in case
/// this runs before settings.rs's own user_data_dir() migration has had a
/// chance to rename that folder forward -- same three names, same order,
/// as db.rs's db_path() and settings.rs's user_data_dir().
fn adb_fallback_path() -> PathBuf {
    let home = dirs::home_dir().expect("no home directory");
    #[cfg(target_os = "windows")]
    let base = home.join("Documents");
    #[cfg(not(target_os = "windows"))]
    let base = home.join(".local").join("share");

    #[cfg(target_os = "windows")]
    let bin_name = "adb.exe";
    #[cfg(not(target_os = "windows"))]
    let bin_name = "adb";

    for dir_name in ["RooQuest", "VRSideForge", "VRRookieDownloader"] {
        let candidate = base.join(dir_name).join("adb").join(bin_name);
        if candidate.exists() {
            return candidate;
        }
    }
    base.join("RooQuest").join("adb").join(bin_name)
}

/// Resolves the adb binary to use: PATH's `adb` if it runs, else the
/// bundled fallback path if it exists, else just "adb" (so the caller's
/// own error at least reads like a normal "command not found" rather than
/// us silently swallowing the resolution failure).
async fn adb_command() -> String {
    if Command::new("adb")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return "adb".to_string();
    }
    let fallback = adb_fallback_path();
    if fallback.exists() {
        return fallback.to_string_lossy().to_string();
    }
    "adb".to_string()
}

fn device_args(device_id: Option<&str>) -> Vec<String> {
    match device_id {
        Some(id) => vec!["-s".to_string(), id.to_string()],
        None => vec![],
    }
}

pub async fn get_adb_devices() -> Vec<String> {
    let cmd = adb_command().await;
    let Ok(out) = Command::new(&cmd).arg("devices").output().await else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .filter(|l| l.contains("\tdevice"))
        .filter_map(|l| l.split('\t').next())
        .map(|s| s.trim().to_string())
        .collect()
}

pub async fn check_adb_path() -> bool {
    let cmd = adb_command().await;
    let Ok(out) = Command::new(&cmd).arg("--version").output().await else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout).contains("Android Debug Bridge version")
}

pub async fn get_installed_apps(device_id: Option<&str>) -> Vec<String> {
    let cmd = adb_command().await;
    let mut args = device_args(device_id);
    args.extend(["shell", "pm", "list", "packages", "-3"].iter().map(|s| s.to_string()));
    let Ok(out) = Command::new(&cmd).args(&args).output().await else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.replace("package:", "").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct StorageInfo {
    pub total: String,
    pub used: String,
    pub free: String,
    pub percentage: String,
}

pub async fn get_storage_info(device_id: Option<&str>) -> Option<StorageInfo> {
    let cmd = adb_command().await;
    let mut args = device_args(device_id);
    args.extend(["shell", "df", "-h", "/storage/emulated"].iter().map(|s| s.to_string()));
    let out = Command::new(&cmd).args(&args).output().await.ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() > 1 {
        let parts: Vec<&str> = lines[lines.len() - 1].trim().split_whitespace().collect();
        if parts.len() >= 5 {
            return Some(StorageInfo {
                total: parts[1].to_string(),
                used: parts[2].to_string(),
                free: parts[3].to_string(),
                percentage: parts[4].to_string(),
            });
        }
    }
    None
}

pub async fn uninstall_app(pkg: &str, device_id: Option<&str>) -> bool {
    let cmd = adb_command().await;
    let mut args = device_args(device_id);
    args.push("uninstall".to_string());
    args.push(pkg.to_string());
    let Ok(out) = Command::new(&cmd).args(&args).output().await else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout).contains("Success")
}

pub async fn pull_path(remote_path: &str, local_path: &str, device_id: Option<&str>) -> Result<(), String> {
    let cmd = adb_command().await;
    let mut args = device_args(device_id);
    args.push("pull".to_string());
    args.push(remote_path.to_string());
    args.push(local_path.to_string());
    let out = Command::new(&cmd).args(&args).output().await.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

pub async fn push_path(local_path: &str, remote_path: &str, device_id: Option<&str>) -> Result<(), String> {
    let cmd = adb_command().await;
    let mut args = device_args(device_id);
    args.push("push".to_string());
    args.push(local_path.to_string());
    args.push(remote_path.to_string());
    let out = Command::new(&cmd).args(&args).output().await.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

/// Runs an arbitrary adb subcommand (used by install.txt's custom lines).
/// Mirrors `runAdbCommand()`'s stripping of a leading "adb " if present.
/// NOTE: unlike the Node version (which passes the whole string to a shell),
/// this splits on whitespace -- a custom install.txt line containing a
/// quoted argument with spaces (e.g. a path) will NOT be parsed correctly.
/// That's a real behavior gap versus the original, not yet addressed.
pub async fn run_adb_command(command: &str, device_id: Option<&str>) -> Result<(String, String), String> {
    let cmd = adb_command().await;
    let clean = command.strip_prefix("adb ").unwrap_or(command);
    let mut args = device_args(device_id);
    args.extend(clean.split_whitespace().map(|s| s.to_string()));
    let out = Command::new(&cmd).args(&args).output().await.map_err(|e| e.to_string())?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    ))
}

pub struct InstallResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Port of `installApp()`: push the APK to a temp path on-device, `pm
/// install -r -g` it, then clean up the temp file. Reports progress via
/// `on_step` (1 = pushing, 2 = installing, 3 = cleaning up), the same three
/// steps the Node version reports.
pub async fn install_app(apk_path: &str, device_id: Option<&str>, mut on_step: impl FnMut(u8)) -> InstallResult {
    let cmd = adb_command().await;
    let absolute = std::fs::canonicalize(apk_path).unwrap_or_else(|_| PathBuf::from(apk_path));
    let remote_temp = format!("/data/local/tmp/temp_install_{}.apk", chrono::Utc::now().timestamp_millis());

    on_step(1);
    let mut push_args = device_args(device_id);
    push_args.push("push".to_string());
    push_args.push(absolute.to_string_lossy().to_string());
    push_args.push(remote_temp.clone());
    let push_out = match Command::new(&cmd).args(&push_args).output().await {
        Ok(o) => o,
        Err(e) => return InstallResult { success: false, stdout: String::new(), stderr: e.to_string() },
    };
    let push_combined = format!(
        "{}{}",
        String::from_utf8_lossy(&push_out.stdout),
        String::from_utf8_lossy(&push_out.stderr)
    );
    if !push_out.status.success() || push_combined.contains("error:") {
        return InstallResult { success: false, stdout: String::new(), stderr: push_combined };
    }

    on_step(2);
    let (stdout, stderr) = match run_adb_command(&format!("shell pm install -r -g {remote_temp}"), device_id).await {
        Ok(v) => v,
        Err(e) => return InstallResult { success: false, stdout: String::new(), stderr: e },
    };
    let success = stdout.contains("Success");

    on_step(3);
    let _ = run_adb_command(&format!("shell rm {remote_temp}"), device_id).await;

    InstallResult { success, stdout, stderr }
}

/// Port of `pushObb()`: wipes and recreates the device-side OBB folder for
/// `pkg` (best-effort, errors ignored -- same as the Node version), then
/// pushes the local `obb_dir`'s contents into it.
pub async fn push_obb(obb_dir: &str, pkg: &str, device_id: Option<&str>, mut on_step: impl FnMut(u8)) -> Result<(), String> {
    let cmd = adb_command().await;
    let target_parent = "/storage/emulated/0/Android/obb/";
    let target_path = format!("{target_parent}{pkg}");
    let absolute_obb_dir = std::fs::canonicalize(obb_dir).unwrap_or_else(|_| PathBuf::from(obb_dir));

    on_step(1);
    let _ = run_adb_command(&format!("shell rm -rf \"{target_path}\""), device_id).await;
    let _ = run_adb_command(&format!("shell mkdir -p \"{target_path}\""), device_id).await;

    let mut args = device_args(device_id);
    args.push("push".to_string());
    args.push(absolute_obb_dir.to_string_lossy().to_string());
    args.push(target_parent.to_string());
    let out = Command::new(&cmd).args(&args).output().await.map_err(|e| e.to_string())?;
    if out.status.success() {
        on_step(2);
        Ok(())
    } else {
        Err(format!("adb push failed: {}", String::from_utf8_lossy(&out.stderr)))
    }
}

/// Port of `getApkPackageName()`: shells out to `aapt`/`aapt2 dump badging`.
/// NOTE: this depends on aapt/aapt2 being on PATH, same as the Node version
/// did -- neither is bundled with this app. If most users don't have the
/// Android SDK build-tools installed, this will usually return `None` and
/// callers fall back to the filename; a pure-Rust APK manifest parser would
/// close that gap but hasn't been written.
pub async fn get_apk_package_name(apk_path: &str) -> Option<String> {
    let absolute = std::fs::canonicalize(apk_path).unwrap_or_else(|_| PathBuf::from(apk_path));
    for tool in ["aapt", "aapt2"] {
        if let Ok(out) = Command::new(tool).arg("dump").arg("badging").arg(&absolute).output().await {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Some(pkg) = extract_package_name(&stdout) {
                return Some(pkg);
            }
        }
    }
    None
}

fn extract_package_name(aapt_output: &str) -> Option<String> {
    let marker = "package: name='";
    let start = aapt_output.find(marker)? + marker.len();
    let end = aapt_output[start..].find('\'')? + start;
    Some(aapt_output[start..end].to_string())
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct BackupResult {
    pub pkg: String,
    pub apk_backed_up: bool,
    pub obb_backed_up: bool,
    pub backup_dir: String,
}

/// Resolves the on-device path of an installed package's APK via `pm path`,
/// e.g. "package:/data/app/~~.../base.apk" -> "/data/app/~~.../base.apk".
/// A split APK (app bundle) can report more than one "package:" line; we take
/// the first, which is the base APK in every case we've seen.
pub async fn get_apk_remote_path(pkg: &str, device_id: Option<&str>) -> Option<String> {
    let (stdout, _stderr) = run_adb_command(&format!("shell pm path {pkg}"), device_id).await.ok()?;
    stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix("package:").map(|s| s.trim().to_string()))
}

/// Manual, on-demand backup of one installed app: pulls its APK and its OBB
/// folder (if any) from the device into `<local_backup_root>/<pkg>/`. This is
/// never run automatically -- only in direct response to the user clicking a
/// "Back up" button for a specific app, per the explicit "manual not
/// automatic" requirement. Each piece (APK, OBB) is attempted independently
/// and its own success is reported, since a game with no OBB data is a normal
/// case, not a failure.
pub async fn backup_app(pkg: &str, device_id: Option<&str>, local_backup_root: &str) -> Result<BackupResult, String> {
    let dest_dir = PathBuf::from(local_backup_root).join(pkg);
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("couldn't create backup folder: {e}"))?;

    let apk_backed_up = match get_apk_remote_path(pkg, device_id).await {
        Some(remote_apk) => {
            let local_apk = dest_dir.join("app.apk");
            pull_path(&remote_apk, &local_apk.to_string_lossy(), device_id).await.is_ok()
        }
        None => false,
    };

    let remote_obb = format!("/storage/emulated/0/Android/obb/{pkg}");
    let local_obb_dir = dest_dir.join("obb");
    let obb_backed_up = pull_path(&remote_obb, &local_obb_dir.to_string_lossy(), device_id).await.is_ok();

    if !apk_backed_up && !obb_backed_up {
        return Err(format!("could not back up {pkg}: neither the APK nor an OBB folder could be pulled from the device"));
    }

    Ok(BackupResult {
        pkg: pkg.to_string(),
        apk_backed_up,
        obb_backed_up,
        backup_dir: dest_dir.to_string_lossy().to_string(),
    })
}
