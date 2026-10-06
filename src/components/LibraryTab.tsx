import { useEffect, useMemo, useState } from "react";
import { useLibraryStore } from "../store/library";
import { useDownloadsStore } from "../store/downloads";
import { GameCard } from "./GameCard";
import { GameDetailModal } from "./GameDetailModal";
import type { Game } from "../lib/tauri";
import { gamePlatforms, gameSource, releaseYear } from "../lib/display";

type SortKey = "added" | "name" | "year" | "developer";

export function LibraryTab({ onDownloadStarted }: { onDownloadStarted: () => void }) {
  const { games, loading, error, load, toggleWishlist, patchGame } = useLibraryStore();
  const { add: addDownload } = useDownloadsStore();
  const [search, setSearch] = useState("");
  const [platform, setPlatform] = useState("");
  const [source, setSource] = useState("");
  const [sortBy, setSortBy] = useState<SortKey>("added");
  const [selected, setSelected] = useState<Game | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [downloadError, setDownloadError] = useState<string | null>(null);

  useEffect(() => {
    load();
  }, [load]);

  const platforms = useMemo(() => {
    const set = new Set<string>();
    for (const g of games) {
      for (const p of gamePlatforms(g)) set.add(p);
    }
    return Array.from(set).sort((a, b) => a.localeCompare(b));
  }, [games]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    let result = games;
    if (q) {
      result = result.filter((g) => (g.title ?? "").toLowerCase().includes(q));
    }
    if (platform) {
      result = result.filter((g) => gamePlatforms(g).includes(platform));
    }
    if (source) {
      result = result.filter((g) => gameSource(g) === source);
    }

    if (sortBy === "added") return result;

    const sorted = [...result];
    if (sortBy === "name") {
      sorted.sort((a, b) => (a.title ?? "").localeCompare(b.title ?? ""));
    } else if (sortBy === "developer") {
      sorted.sort((a, b) => (a.developer ?? "").localeCompare(b.developer ?? ""));
    } else if (sortBy === "year") {
      sorted.sort((a, b) => (releaseYear(b) ?? 0) - (releaseYear(a) ?? 0));
    }
    return sorted;
  }, [games, search, platform, source, sortBy]);

  const handleDownload = async () => {
    if (!selected?.magnet) return;
    setDownloading(true);
    setDownloadError(null);
    try {
      await addDownload(selected.magnet, selected.title ?? undefined);
      setSelected(null);
      onDownloadStarted();
    } catch (err) {
      // Previously swallowed entirely -- the button would say "Starting…"
      // then silently reset with nothing added to Downloads and no
      // indication why. Surface whatever the backend actually said.
      setDownloadError(String(err));
    } finally {
      setDownloading(false);
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-3 px-8 py-5 border-b border-border flex-wrap">
        <div className="relative flex-1 min-w-[220px] max-w-[420px]">
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            className="absolute left-3.5 top-2.5 opacity-55"
          >
            <circle cx="11" cy="11" r="7" stroke="currentColor" strokeWidth="1.8" />
            <path d="M21 21l-4.3-4.3" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
          </svg>
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search library…"
            aria-label="Search library"
            className="w-full bg-panel2 border border-border rounded-xl text-text text-sm pl-10 pr-3.5 py-2.5 outline-none"
          />
        </div>

        <select
          value={platform}
          onChange={(e) => setPlatform(e.target.value)}
          aria-label="Filter by platform"
          className="bg-panel2 border border-border rounded-xl text-text text-sm px-3 py-2.5 outline-none"
        >
          <option value="">All platforms</option>
          {platforms.map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>

        <select
          value={source}
          onChange={(e) => setSource(e.target.value)}
          aria-label="Filter by source"
          className="bg-panel2 border border-border rounded-xl text-text text-sm px-3 py-2.5 outline-none"
        >
          <option value="">All sources</option>
          <option value="RuTracker">RuTracker</option>
          <option value="NNM-Club">NNM-Club</option>
        </select>

        <select
          value={sortBy}
          onChange={(e) => setSortBy(e.target.value as SortKey)}
          aria-label="Sort by"
          className="bg-panel2 border border-border rounded-xl text-text text-sm px-3 py-2.5 outline-none"
        >
          <option value="added">Recently added</option>
          <option value="name">Name (A–Z)</option>
          <option value="year">Release year (newest)</option>
          <option value="developer">Developer (A–Z)</option>
        </select>

        <div className="flex-1" />
        <button
          onClick={load}
          className="px-3 py-1.5 text-xs rounded-md bg-panel2 text-subtext hover:text-text"
        >
          Refresh
        </button>
      </div>

      <div className="p-8 overflow-y-auto flex-1">
        <div className="flex items-baseline justify-between mb-5">
          <h1 className="text-[22px] font-semibold text-text">Library</h1>
          <span className="text-sm text-subtext">{filtered.length} titles</span>
        </div>

        {loading && <div className="text-subtext text-sm">Loading…</div>}
        {error && <div className="text-warn text-sm">Failed to load library: {error}</div>}
        {!loading && !error && filtered.length === 0 && (
          <div className="text-subtext text-sm">
            No games indexed yet — run a scraper test from Settings, or scan your downloads from
            Sideload.
          </div>
        )}

        <div className="grid gap-5" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(236px, 1fr))" }}>
          {filtered.map((g) => (
            <GameCard
              key={g.id}
              game={g}
              onOpen={() => setSelected(g)}
              onToggleWishlist={() => g.id != null && toggleWishlist(g.id, g.wishlist === 0)}
            />
          ))}
        </div>
      </div>

      {selected && (
        <GameDetailModal
          game={selected}
          onClose={() => setSelected(null)}
          onDownload={handleDownload}
          downloading={downloading}
          downloadError={downloadError}
          onTranslated={(updated) => {
            patchGame(updated);
            setSelected(updated);
          }}
        />
      )}
    </div>
  );
}
