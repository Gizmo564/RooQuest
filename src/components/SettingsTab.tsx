import { useEffect, useState } from "react";
import { useSettingsStore } from "../store/settings";
import { TorrentSourcesPanel } from "./TorrentSourcesPanel";
import { useBackfillStore } from "../store/backfill";

export function SettingsTab() {
  const { settings, loading, error, load, update } = useSettingsStore();

  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved">("idle");

  // Pulled from a module-level store (not local state) so a run started here
  // keeps showing as running -- with live progress -- even after switching
  // to another tab and back. The backend keeps running regardless either
  // way; this just keeps the UI honest about it.
  const {
    igdbRunning,
    igdbProgress,
    igdbResult,
    runIgdb,
    translateRunning,
    translateProgress,
    translateResult,
    runTranslate,
  } = useBackfillStore();

  useEffect(() => {
    load();
  }, [load]);

  if (loading && !settings) return <div className="text-subtext text-sm p-6">Loading settings…</div>;
  if (error) return <div className="text-warn text-sm p-6">Failed to load settings: {error}</div>;
  if (!settings) return null;

  const handleSave = async () => {
    setSaveStatus("saving");
    try {
      await update(settings);
      setSaveStatus("saved");
      setTimeout(() => setSaveStatus("idle"), 2000);
    } catch {
      setSaveStatus("idle");
    }
  };

  return (
    <div className="p-6 max-w-2xl flex flex-col gap-4">
      <h1 className="text-lg font-semibold text-text">Settings</h1>

      <div className="bg-panel border border-border rounded-lg p-4">
        <label className="block text-xs text-subtext mb-1">Download folder</label>
        <input
          value={settings.downloadPath}
          onChange={(e) => update({ downloadPath: e.target.value })}
          className="w-full bg-bg border border-border rounded px-2 py-1.5 text-sm text-text"
        />
        <label className="block text-xs text-subtext mb-1 mt-3">Local backup folder</label>
        <input
          value={settings.backupPath}
          onChange={(e) => update({ backupPath: e.target.value })}
          className="w-full bg-bg border border-border rounded px-2 py-1.5 text-sm text-text"
        />
        <p className="text-xs text-subtext mt-1">
          Used by Sideload's "Back up" action (APK + OBB) — manual only, never automatic.
        </p>
      </div>

      <TorrentSourcesPanel />

      <div className="bg-panel border border-border rounded-lg p-4">
        <h2 className="text-sm font-semibold text-text mb-2">Server</h2>
        <label className="block text-xs text-subtext mb-1">Base URI</label>
        <input
          value={settings.serverBaseUri}
          onChange={(e) => update({ serverBaseUri: e.target.value })}
          placeholder="https://your-server/"
          className="w-full bg-bg border border-border rounded px-2 py-1.5 text-sm text-text mb-3"
        />
        <label className="block text-xs text-subtext mb-1">Password</label>
        <input
          type="password"
          value={settings.serverPassword}
          onChange={(e) => update({ serverPassword: e.target.value })}
          className="w-full bg-bg border border-border rounded px-2 py-1.5 text-sm text-text"
        />
      </div>

      <div className="bg-panel border border-border rounded-lg p-4">
        <h2 className="text-sm font-semibold text-text mb-2">Translation</h2>
        <label className="block text-xs text-subtext mb-1">Target language code</label>
        <input
          value={settings.translationLanguage}
          onChange={(e) => update({ translationLanguage: e.target.value })}
          placeholder="en"
          className="w-full bg-bg border border-border rounded px-2 py-1.5 text-sm text-text mb-3"
        />
        <p className="text-xs text-subtext mb-3">
          Every newly-scraped game's title/description are auto-translated into this language.
          The free translate service used here occasionally rate-limits a big scrape and leaves
          some games untranslated — use this to retry just those, without re-scraping anything.
        </p>
        <button
          onClick={runTranslate}
          disabled={translateRunning}
          className="px-3 py-1.5 text-xs rounded-md bg-panel2 text-text hover:bg-border disabled:opacity-50"
        >
          {translateRunning ? "Retranslating…" : "Retranslate library"}
        </button>
        {translateRunning && translateProgress && (
          <p className="text-xs text-subtext mt-2">
            {translateProgress.status} ({translateProgress.processed}/{translateProgress.total}, updated{" "}
            {translateProgress.updated}, failed {translateProgress.failed})
          </p>
        )}
        {!translateRunning && translateResult && (
          <p className="text-xs text-subtext mt-2">{translateResult}</p>
        )}
        {translateRunning && (
          <p className="text-xs text-subtext mt-2">
            This keeps running in the background — feel free to switch tabs.
          </p>
        )}
      </div>

      <div className="bg-panel border border-border rounded-lg p-4">
        <h2 className="text-sm font-semibold text-text mb-2">IGDB metadata</h2>
        <p className="text-xs text-subtext mb-3">
          Optional. When set, every newly-scraped game automatically gets a best-effort
          IGDB lookup for its description, genre, developer, publisher, and cover art.
          These are Twitch Developer app credentials (IGDB auth runs through Twitch's
          OAuth), kept here and used only for this app's own server-side requests to
          IGDB — never shown or sent anywhere else.
        </p>
        <label className="block text-xs text-subtext mb-1">Client ID</label>
        <input
          value={settings.igdbClientId}
          onChange={(e) => update({ igdbClientId: e.target.value })}
          className="w-full bg-bg border border-border rounded px-2 py-1.5 text-sm text-text mb-3"
        />
        <label className="block text-xs text-subtext mb-1">Client Secret</label>
        <input
          type="password"
          value={settings.igdbClientSecret}
          onChange={(e) => update({ igdbClientSecret: e.target.value })}
          className="w-full bg-bg border border-border rounded px-2 py-1.5 text-sm text-text mb-3"
        />
        <button
          onClick={runIgdb}
          disabled={igdbRunning || !settings.igdbClientId || !settings.igdbClientSecret}
          className="px-3 py-1.5 text-xs rounded-md bg-panel2 text-text hover:bg-border disabled:opacity-50"
        >
          {igdbRunning ? "Scraping IGDB…" : "Scrape IGDB now"}
        </button>
        {igdbRunning && igdbProgress && (
          <p className="text-xs text-subtext mt-2">
            {igdbProgress.status} ({igdbProgress.processed}/{igdbProgress.total}, updated{" "}
            {igdbProgress.updated}, failed {igdbProgress.failed})
          </p>
        )}
        {!igdbRunning && igdbResult && <p className="text-xs text-subtext mt-2">{igdbResult}</p>}
      </div>

      <div className="flex items-center gap-3 sticky bottom-0 bg-bg py-3">
        <button
          onClick={handleSave}
          disabled={saveStatus === "saving"}
          className="px-4 py-2 text-sm font-semibold rounded-lg bg-accent text-bg disabled:opacity-60"
        >
          {saveStatus === "saving" ? "Saving…" : saveStatus === "saved" ? "Saved ✓" : "Save"}
        </button>
        <span className="text-xs text-subtext">
          Every field above already saves itself the moment you change it — this button just
          re-confirms everything is written to disk.
        </span>
      </div>
    </div>
  );
}
