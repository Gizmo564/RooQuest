import { useEffect, useRef, useState } from "react";
import {
  onScraperProgress,
  startNnmclubScraperTest,
  startScraperTest,
  type ScraperProgress,
  type SourceLabel,
} from "../lib/tauri";
import { useSourcesStore, type SourceStatus } from "../store/sources";
import { useSettingsStore } from "../store/settings";

const SOURCE_NAMES: Record<SourceLabel, string> = {
  main: "RuTracker",
  nnmclub: "NNM-Club",
  server: "Server",
};

function statusColor(status: SourceStatus): string {
  switch (status) {
    case "logged_in":
      return "text-ok";
    case "logged_out":
    case "error":
      return "text-warn";
    case "blocked":
      return "text-blocked";
    default:
      return "text-subtext";
  }
}

// Only RuTracker and NNM-Club actually have a scraper to automate -- the
// Server source (see sources.rs) has no scrape command at all, so it gets
// no auto-scrape toggle.
const AUTO_SCRAPE_KEY: Partial<Record<SourceLabel, "autoScrapeRutracker" | "autoScrapeNnmclub">> = {
  main: "autoScrapeRutracker",
  nnmclub: "autoScrapeNnmclub",
};

function SourceRow({ source }: { source: SourceLabel }) {
  const { status, message, login, check } = useSourcesStore();
  const { settings, update } = useSettingsStore();
  const autoScrapeKey = AUTO_SCRAPE_KEY[source];

  if (source === "server") {
    return (
      <div className="flex items-center justify-between py-3 border-b border-border last:border-0">
        <div>
          <div className="text-sm text-text">{SOURCE_NAMES.server}</div>
          <div className={`text-xs ${statusColor("blocked")}`}>{message.server}</div>
        </div>
        <button
          onClick={() => login("server")}
          className="px-3 py-1.5 text-xs rounded-md bg-panel2 text-subtext hover:text-text transition-colors"
        >
          Open window
        </button>
      </div>
    );
  }

  return (
    <div className="flex items-center justify-between py-3 border-b border-border last:border-0">
      <div>
        <div className="text-sm text-text">{SOURCE_NAMES[source]}</div>
        <div className={`text-xs ${statusColor(status[source])}`}>{message[source] || "—"}</div>
      </div>
      <div className="flex items-center gap-3">
        {autoScrapeKey && settings && (
          <label className="flex items-center gap-1.5 text-xs text-subtext cursor-pointer select-none">
            <input
              type="checkbox"
              checked={settings[autoScrapeKey]}
              onChange={(e) => update({ [autoScrapeKey]: e.target.checked })}
              className="accent-accent"
            />
            Auto-scrape
          </label>
        )}
        <button
          onClick={() => login(source)}
          className="px-3 py-1.5 text-xs rounded-md bg-panel2 text-subtext hover:text-text transition-colors"
        >
          Log in
        </button>
        <button
          onClick={() => check(source)}
          disabled={status[source] === "checking"}
          className="px-3 py-1.5 text-xs rounded-md bg-panel2 text-subtext hover:text-text transition-colors disabled:opacity-50"
        >
          {status[source] === "checking" ? "Checking…" : "Check login"}
        </button>
      </div>
    </div>
  );
}

export function TorrentSourcesPanel() {
  const [advancedKeyword, setAdvancedKeyword] = useState("");
  const [output, setOutput] = useState<string | null>(null);
  const [running, setRunning] = useState<SourceLabel | null>(null);
  // Live counters from the backend's "scraper_progress" events -- a full
  // scrape walks many pages (RuTracker) or many per-genre forum searches
  // (NNM-Club) and can take a while, so without this the button just sits
  // on "Scraping…" with no sign anything beyond the very first item is
  // happening. runningRef mirrors `running` for the listener closure
  // below, which is set up once on mount rather than re-subscribed per run.
  const [progress, setProgress] = useState<ScraperProgress | null>(null);
  const runningRef = useRef<SourceLabel | null>(null);
  runningRef.current = running;

  useEffect(() => {
    const unlisten = onScraperProgress((p) => {
      if (runningRef.current !== null) {
        setProgress(p);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const runRutracker = async () => {
    setRunning("main");
    setProgress(null);
    setOutput(null);
    try {
      setOutput(await startScraperTest());
    } catch (err) {
      setOutput(`error: ${err}`);
    } finally {
      setRunning(null);
    }
  };

  // No keyword by default -- the backend's own default ("VR", searched
  // across every PC-game genre forum, see start_nnmclub_scraper_test) is
  // the actual full-catalog scrape. This used to always pass whatever was
  // typed in a "NNM-Club keyword" box (defaulting to a single game's
  // title!), which meant the manual-scrape button was really a one-game
  // lookup tool, not a scrape of the site -- that's why it only ever
  // turned up "one game at a time". The keyword field below is now an
  // opt-in advanced option for that one-game lookup, separate from this.
  const runNnmclub = async () => {
    setRunning("nnmclub");
    setProgress(null);
    setOutput(null);
    try {
      setOutput(await startNnmclubScraperTest(advancedKeyword));
    } catch (err) {
      setOutput(`error: ${err}`);
    } finally {
      setRunning(null);
    }
  };

  return (
    <div className="bg-panel border border-border rounded-lg p-4">
      <h2 className="text-sm font-semibold text-text mb-2">Torrent sources</h2>
      <SourceRow source="main" />
      <SourceRow source="nnmclub" />
      <SourceRow source="server" />

      <h3 className="text-xs font-semibold text-subtext uppercase tracking-wide mt-4 mb-2">
        Manual scrape
      </h3>
      <p className="text-xs text-subtext mb-3">
        Log in to a source above first. Use these any time, independent of "Auto-scrape" above --
        turning auto-scrape on doesn't stop you from also triggering a scrape by hand. Either one
        walks the ENTIRE listing/catalog for that source, which can take a while -- the counters
        below update live as it goes.
      </p>
      <div className="flex items-center gap-2 flex-wrap mb-2">
        <button
          onClick={runRutracker}
          disabled={running !== null}
          className="px-3 py-1.5 text-xs rounded-md bg-accent text-white disabled:opacity-50"
        >
          {running === "main" ? "Scraping RuTracker…" : "Scrape RuTracker now"}
        </button>
        <button
          onClick={runNnmclub}
          disabled={running !== null}
          className="px-3 py-1.5 text-xs rounded-md bg-accent text-white disabled:opacity-50"
        >
          {running === "nnmclub" ? "Scraping NNM-Club…" : "Scrape NNM-Club now"}
        </button>
      </div>

      <details className="mb-3">
        <summary className="text-xs text-subtext cursor-pointer select-none">
          Advanced: look up one NNM-Club title by name instead
        </summary>
        <label className="flex items-center gap-1.5 text-xs text-subtext mt-2">
          Keyword (leave blank for the normal full scrape above):
          <input
            value={advancedKeyword}
            onChange={(e) => setAdvancedKeyword(e.target.value)}
            placeholder="e.g. Beat Saber"
            className="bg-bg border border-border rounded px-2 py-1 text-text w-40"
          />
        </label>
      </details>

      {running !== null && (
        <div className="bg-bg border border-border rounded-md p-3 text-xs text-text mb-2">
          <div className="mb-1">{progress?.status ?? "Starting…"}</div>
          <div className="text-subtext">
            saved {progress?.saved ?? 0} · skipped {progress?.skipped ?? 0} · failed{" "}
            {progress?.failed ?? 0}
          </div>
        </div>
      )}

      {output !== null && (
        <pre className="bg-bg border border-border rounded-md p-3 text-xs text-muted whitespace-pre-wrap break-words">
          {output}
        </pre>
      )}
    </div>
  );
}
