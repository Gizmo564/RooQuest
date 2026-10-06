import { useEffect, useState } from "react";
import { Sidebar, type Tab } from "./components/Sidebar";
import { LibraryTab } from "./components/LibraryTab";
import { DownloadsTab } from "./components/DownloadsTab";
import { SideloadTab } from "./components/SideloadTab";
import { SettingsTab } from "./components/SettingsTab";
import { useDownloadsStore, isPaused } from "./store/downloads";
import { useUiStore } from "./store/ui";
import { useBackfillStore } from "./store/backfill";

export default function App() {
  const [tab, setTab] = useState<Tab>("library");
  const { torrents, refresh } = useDownloadsStore();
  const sideloadBusy = useUiStore((s) => s.sideloadBusy);
  const subscribeBackfill = useBackfillStore((s) => s.subscribe);

  // Subscribed once here (not inside SettingsTab) so IGDB/translate backfill
  // progress keeps flowing into the store -- and a run keeps showing as
  // running -- no matter which tab is actually open.
  useEffect(() => {
    subscribeBackfill();
  }, [subscribeBackfill]);

  // Keep the sidebar's active-download badge live even while on another
  // tab (DownloadsTab itself polls faster while actually open).
  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 4000);
    return () => clearInterval(id);
  }, [refresh]);

  const activeTorrentCount = torrents.filter((t) => !isPaused(t.state)).length;

  return (
    <div className="flex h-screen bg-bg text-text">
      <Sidebar
        active={tab}
        onChange={setTab}
        activeTorrentCount={activeTorrentCount}
        sideloadInstalling={sideloadBusy}
      />
      <main className="flex-1 overflow-y-auto">
        {tab === "library" && <LibraryTab onDownloadStarted={() => setTab("downloads")} />}
        {tab === "downloads" && <DownloadsTab />}
        {tab === "sideload" && <SideloadTab />}
        {tab === "settings" && <SettingsTab />}
      </main>
    </div>
  );
}
