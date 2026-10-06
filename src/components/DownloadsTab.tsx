import { useEffect } from "react";
import { useDownloadsStore, isPaused } from "../store/downloads";

export function DownloadsTab() {
  const { torrents, refresh, togglePause, remove } = useDownloadsStore();

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 2000);
    return () => clearInterval(id);
  }, [refresh]);

  return (
    <div className="p-8">
      <h1 className="text-[22px] font-semibold text-text mb-5">Downloads</h1>

      {torrents.length === 0 && (
        <div className="flex flex-col items-center gap-2.5 py-20 text-subtext">
          <svg width="30" height="30" viewBox="0 0 24 24" fill="none">
            <path
              d="M12 3v12m0 0l-4.5-4.5M12 15l4.5-4.5"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          <span className="text-sm">No active downloads</span>
        </div>
      )}

      <div className="flex flex-col gap-3">
        {torrents.map((t) => {
          const paused = isPaused(t.state);
          return (
            <div key={t.hash} className="bg-panel border border-border rounded-xl px-5 py-4">
              <div className="flex items-center gap-3 mb-2.5">
                <div className="font-semibold text-sm flex-1 min-w-0 truncate text-text">{t.name}</div>
                <span className="text-[11px] font-semibold text-muted bg-panel2 rounded-full px-2.5 py-0.5 flex-none">
                  {t.engine}
                </span>
                <span className={`text-xs flex-none ${paused ? "text-blocked" : "text-ok"}`}>
                  {paused ? "Paused" : t.state}
                </span>
                <button
                  aria-label="Toggle pause"
                  onClick={() => togglePause(t)}
                  className="w-[30px] h-[30px] rounded-lg bg-panel2 text-text flex items-center justify-center flex-none"
                >
                  {paused ? (
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
                      <path d="M8 5l11 7-11 7z" />
                    </svg>
                  ) : (
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
                      <path d="M7 5h4v14H7zM13 5h4v14h-4z" />
                    </svg>
                  )}
                </button>
                <button
                  aria-label="Remove torrent"
                  onClick={() => remove(t.hash)}
                  className="w-[30px] h-[30px] rounded-lg bg-panel2 text-warn flex items-center justify-center flex-none"
                >
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none">
                    <path
                      d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2m-9 0l1 12a2 2 0 0 0 2 2h4a2 2 0 0 0 2-2l1-12"
                      stroke="currentColor"
                      strokeWidth="1.8"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                </button>
              </div>
              <div className="h-1.5 rounded-full bg-panel2 overflow-hidden">
                <div
                  className="h-full rounded-full bg-accent transition-[width]"
                  style={{ width: `${t.progress}%` }}
                />
              </div>
              <div className="flex justify-between mt-1.5 text-[11.5px] text-subtext">
                <span>{t.progress}%</span>
                <span>
                  {t.peers_connected} peer{t.peers_connected === 1 ? "" : "s"}
                  {t.peers_seen > t.peers_connected ? ` (${t.peers_seen} seen)` : ""}
                  {" · "}
                  {t.download_speed} down · {t.upload_speed} up
                </span>
              </div>
              {t.error && (
                <p className="text-[11.5px] text-warn mt-1.5">
                  {t.engine} error: {t.error}
                </p>
              )}
              {!t.error && t.peers_connected === 0 && t.progress === 0 && !paused && (
                <p className="text-[11.5px] text-subtext mt-1.5">
                  {t.peers_seen === 0
                    ? "No peers found yet (checking DHT/trackers) — stuck here usually means no reachable peers for this torrent, or an outbound-connection/firewall issue."
                    : `Found ${t.peers_seen} peer(s) but none connected yet.`}
                </p>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
