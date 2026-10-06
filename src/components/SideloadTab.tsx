import { useEffect, useState } from "react";
import {
  adbApps,
  adbBackupApp,
  adbCheckPath,
  adbDevices,
  adbInstall,
  adbUninstall,
  fsFolders,
  fsManualIndex,
  fsRemoveIndex,
  fsScanLocalDownloads,
  type AdbAppsResult,
  type FolderInfo,
  type StorageInfo,
} from "../lib/tauri";
import { useUiStore } from "../store/ui";

// Parses df -h style strings ("11G", "58.3G", "1.2T", "812M") into a rough
// GiB figure purely for the bar's fill ratio -- display still uses the
// device's own formatted strings (StorageInfo.total/used/free/percentage),
// this is only ever used as a number to size the bar.
function parseSizeToGiB(raw: string): number {
  const m = raw.trim().match(/^([\d.]+)\s*([KMGT]?)/i);
  if (!m) return 0;
  const value = parseFloat(m[1]);
  if (Number.isNaN(value)) return 0;
  const unit = m[2].toUpperCase();
  switch (unit) {
    case "T":
      return value * 1024;
    case "M":
      return value / 1024;
    case "K":
      return value / 1024 / 1024;
    default:
      return value; // already G, or unitless (treat as G -- adb's df -h always has a unit in practice)
  }
}

function StorageBar({ storage }: { storage: StorageInfo }) {
  // Prefer the device's own reported percentage (e.g. "72%"); only fall
  // back to computing one from the size strings if that's ever missing or
  // unparseable, so the bar never just goes blank.
  const parsedPct = parseFloat(storage.percentage);
  let pct = Number.isNaN(parsedPct) ? null : parsedPct;
  if (pct === null) {
    const total = parseSizeToGiB(storage.total);
    const used = parseSizeToGiB(storage.used);
    pct = total > 0 ? (used / total) * 100 : 0;
  }
  pct = Math.min(100, Math.max(0, pct));

  const barColor = pct < 70 ? "bg-ok" : pct < 90 ? "bg-blocked" : "bg-warn";

  return (
    <div className="bg-panel border border-border rounded-lg p-4">
      <div className="flex items-baseline justify-between mb-2">
        <h2 className="text-sm font-semibold text-text">Headset storage</h2>
        <span className="text-xs text-subtext">
          {storage.used} used / {storage.total} total
        </span>
      </div>
      <div className="h-2.5 rounded-full bg-panel2 overflow-hidden">
        <div
          className={`h-full rounded-full ${barColor}`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className="flex items-baseline justify-between mt-1.5 text-[11px] text-subtext">
        <span>{pct.toFixed(0)}% full</span>
        <span>{storage.free} free</span>
      </div>
      {pct >= 90 && (
        <p className="text-[11px] text-warn mt-1.5">
          Headset storage is nearly full — installs or OBB pushes may fail until something's
          freed up.
        </p>
      )}
    </div>
  );
}

export function SideloadTab() {
  const [hasAdb, setHasAdb] = useState<boolean | null>(null);
  const [devices, setDevices] = useState<string[]>([]);
  const [deviceId, setDeviceId] = useState<string | undefined>(undefined);
  const [appsResult, setAppsResult] = useState<AdbAppsResult | null>(null);
  const [folders, setFolders] = useState<FolderInfo[]>([]);
  const [busyPkg, setBusyPkg] = useState<string | null>(null);
  const setSideloadBusy = useUiStore((st) => st.setSideloadBusy);
  const [backedUp, setBackedUp] = useState<Record<string, string>>({});
  const [log, setLog] = useState<string[]>([]);

  const pushLog = (line: string) => setLog((l) => [line, ...l].slice(0, 20));

  const refreshDevices = async () => {
    const ok = await adbCheckPath();
    setHasAdb(ok);
    if (!ok) return;
    const list = await adbDevices();
    setDevices(list);
    if (list.length && !deviceId) setDeviceId(list[0]);
  };

  const refreshApps = async () => {
    if (!hasAdb) return;
    setAppsResult(await adbApps(deviceId));
  };

  const refreshFolders = async () => {
    setFolders(await fsFolders());
  };

  useEffect(() => {
    refreshDevices();
    refreshFolders();
  }, []);

  useEffect(() => {
    if (hasAdb) refreshApps();
  }, [hasAdb, deviceId]);

  const handleUninstall = async (pkg: string) => {
    setBusyPkg(pkg);
    setSideloadBusy(true);
    try {
      const ok = await adbUninstall(pkg, deviceId);
      pushLog(ok ? `Uninstalled ${pkg}` : `Failed to uninstall ${pkg}`);
      await refreshApps();
    } finally {
      setBusyPkg(null);
      setSideloadBusy(false);
    }
  };

  const handleBackup = async (pkg: string) => {
    setBusyPkg(pkg);
    setSideloadBusy(true);
    try {
      const result = await adbBackupApp(pkg, deviceId);
      setBackedUp((b) => ({ ...b, [pkg]: result.backup_dir }));
      pushLog(
        `Backed up ${pkg}${result.apk_backed_up ? " (apk)" : ""}${
          result.obb_backed_up ? " (obb)" : ""
        } → ${result.backup_dir}`
      );
    } catch (err) {
      pushLog(`Backup failed for ${pkg}: ${err}`);
    } finally {
      setBusyPkg(null);
      setSideloadBusy(false);
    }
  };

  const handleScan = async () => {
    const result = await fsScanLocalDownloads();
    pushLog(`Scan: matched ${result.matchedCount}/${result.totalScanned}`);
    await refreshFolders();
  };

  const handleInstall = async (folder: FolderInfo) => {
    setBusyPkg(folder.name);
    setSideloadBusy(true);
    try {
      await adbInstall(folder.name, deviceId);
      pushLog(`Installed from ${folder.name}`);
    } catch (err) {
      pushLog(`Install failed for ${folder.name}: ${err}`);
    } finally {
      setBusyPkg(null);
      setSideloadBusy(false);
    }
  };

  // Only actually finished downloads -- a folder whose matching torrent is
  // still in progress (downloadProgress set) has an .apk that exists on
  // disk but isn't fully written yet, and installing it would push a
  // truncated/corrupt file to the headset.
  const readyToInstall = folders.filter((f) => f.hasApk && f.downloadProgress == null);
  const stillDownloading = folders.filter((f) => f.hasApk && f.downloadProgress != null);

  return (
    <div className="p-6 flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold text-text">Sideload</h1>
        {devices.length > 1 && (
          <select
            value={deviceId}
            onChange={(e) => setDeviceId(e.target.value)}
            className="bg-panel border border-border rounded px-2 py-1 text-xs text-text"
          >
            {devices.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        )}
      </div>

      {hasAdb === false && (
        <div className="bg-panel border border-border rounded-lg p-4 text-sm text-warn">
          ADB not found on this machine — install Android Platform Tools to use device
          install/uninstall/backup.
        </div>
      )}
      {hasAdb && devices.length === 0 && (
        <div className="bg-panel border border-border rounded-lg p-4 text-sm text-subtext">
          No device detected — plug one in with USB debugging enabled.
        </div>
      )}

      {appsResult?.storage && <StorageBar storage={appsResult.storage} />}

      {/* Downloaded, ready to install -- carried over from the RooQuest design's
          Sideload section: local folders with an APK present get a direct
          Install button instead of needing to go hunt for the file. */}
      {readyToInstall.length > 0 && (
        <div className="bg-panel border border-border rounded-lg p-4">
          <h2 className="text-sm font-semibold text-text mb-2">Downloaded, ready to install</h2>
          <div className="flex flex-col gap-2">
            {readyToInstall.map((f) => (
              <div key={f.name} className="flex items-center justify-between text-sm">
                <span className="text-text">{f.name}</span>
                <button
                  onClick={() => handleInstall(f)}
                  disabled={busyPkg === f.name}
                  className="px-3 py-1.5 text-xs rounded-md bg-accent text-white disabled:opacity-50"
                >
                  {busyPkg === f.name ? "Installing…" : "Install"}
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Still downloading -- has an .apk on disk already (torrent clients
          pre-allocate the file), but the matching torrent isn't at 100% yet,
          so it's shown here instead of getting an Install button. */}
      {stillDownloading.length > 0 && (
        <div className="bg-panel border border-border rounded-lg p-4">
          <h2 className="text-sm font-semibold text-text mb-2">Still downloading</h2>
          <div className="flex flex-col gap-2">
            {stillDownloading.map((f) => (
              <div key={f.name} className="flex items-center justify-between text-sm">
                <span className="text-subtext">{f.name}</span>
                <span className="text-xs text-subtext">{f.downloadProgress?.toFixed(1)}%</span>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="bg-panel border border-border rounded-lg p-4">
        <div className="flex items-center justify-between mb-2">
          <h2 className="text-sm font-semibold text-text">Installed apps</h2>
          <button
            onClick={handleScan}
            className="px-3 py-1.5 text-xs rounded-md bg-panel2 text-subtext hover:text-text"
          >
            Scan downloads
          </button>
        </div>
        <div className="flex flex-col gap-2">
          {(appsResult?.apps ?? []).map((pkg) => (
            <div key={pkg} className="flex items-center justify-between text-sm">
              <span className="text-text">{pkg}</span>
              <div className="flex gap-2 items-center">
                {backedUp[pkg] && <span className="text-xs text-ok">backed up</span>}
                <button
                  onClick={() => handleBackup(pkg)}
                  disabled={busyPkg === pkg}
                  className="px-3 py-1.5 text-xs rounded-md bg-panel2 text-subtext hover:text-text disabled:opacity-50"
                >
                  Back up
                </button>
                <button
                  onClick={() => handleUninstall(pkg)}
                  disabled={busyPkg === pkg}
                  className="px-3 py-1.5 text-xs rounded-md bg-panel2 text-subtext hover:text-warn disabled:opacity-50"
                >
                  Uninstall
                </button>
              </div>
            </div>
          ))}
          {hasAdb && (appsResult?.apps ?? []).length === 0 && (
            <div className="text-xs text-subtext">No third-party apps found.</div>
          )}
        </div>
      </div>

      <div className="bg-panel border border-border rounded-lg p-4">
        <h2 className="text-sm font-semibold text-text mb-2">Activity log</h2>
        <div className="flex flex-col gap-1 text-xs text-muted max-h-40 overflow-y-auto">
          {log.length === 0 && <div className="text-subtext">(nothing yet)</div>}
          {log.map((l, i) => (
            <div key={i}>{l}</div>
          ))}
        </div>
      </div>
    </div>
  );
}
