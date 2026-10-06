import { useState } from "react";
import { translateOne, type Game } from "../lib/tauri";
import { coverGradient, gameSynopsis, gameTags } from "../lib/display";

export function GameDetailModal({
  game,
  onClose,
  onDownload,
  downloading,
  downloadError,
  onTranslated,
}: {
  game: Game;
  onClose: () => void;
  onDownload: () => void;
  downloading: boolean;
  downloadError?: string | null;
  // Lets the parent (which owns the actual library list) patch this game's
  // translated fields everywhere once a one-off translation finishes --
  // this component only holds the "is it running" state, not the data.
  onTranslated?: (game: Game) => void;
}) {
  const hasMagnet = !!game.magnet && game.magnet.startsWith("magnet:");

  // A game counts as untranslated if it has a description at all but the
  // translated copy is missing or identical to the raw text -- the same
  // signal translate_backfill_all uses to decide what still needs work.
  const isUntranslated =
    !!game.description &&
    (!game.translated_description || game.translated_description === game.description);

  const [translating, setTranslating] = useState(false);
  const [translateError, setTranslateError] = useState<string | null>(null);

  const handleTranslateNow = async () => {
    if (game.id == null) return;
    setTranslating(true);
    setTranslateError(null);
    try {
      const updated = await translateOne(game.id);
      onTranslated?.(updated);
    } catch (err) {
      setTranslateError(String(err));
    } finally {
      setTranslating(false);
    }
  };

  return (
    <div
      onClick={onClose}
      className="fixed inset-0 bg-black/70 flex items-center justify-center z-50"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-[560px] max-w-[90vw] max-h-[85vh] bg-panel border border-border rounded-2xl overflow-hidden flex flex-col"
      >
        <div
          className="h-[180px] shrink-0 relative flex items-center justify-center"
          style={game.image_url ? undefined : { background: coverGradient(game.id) }}
        >
          {game.image_url ? (
            <img
              src={game.image_url}
              alt=""
              className="absolute inset-0 w-full h-full object-cover"
            />
          ) : (
            <svg width="44" height="44" viewBox="0 0 24 24" fill="none" className="opacity-35">
              <rect x="3" y="7" width="18" height="11" rx="4" stroke="white" strokeWidth="1.6" />
              <circle cx="8.5" cy="12.5" r="1.6" fill="white" />
              <circle cx="15.5" cy="12.5" r="1.6" fill="white" />
            </svg>
          )}
          <button
            aria-label="Close"
            onClick={onClose}
            className="absolute top-3 right-3 w-[30px] h-[30px] rounded-full bg-black/50 flex items-center justify-center text-text"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
              <path d="M6 6l12 12M18 6L6 18" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
            </svg>
          </button>
        </div>

        {/* Scrollable body -- title/tags/synopsis/stats can run long (untranslated
            descriptions especially), so this area scrolls independently and the
            Download button below stays out of it, always visible without scrolling. */}
        <div className="p-6 overflow-y-auto flex-1 min-h-0">
          <div className="text-[19px] font-semibold mb-2 text-text">{game.title ?? "(untitled)"}</div>
          <div className="flex flex-wrap gap-1.5 mb-3.5">
            {gameTags(game).map((tag) => (
              <span key={tag} className="text-[11px] text-muted bg-panel2 rounded px-2 py-0.5">
                {tag}
              </span>
            ))}
          </div>
          {isUntranslated && (
            <div className="mb-3">
              <button
                onClick={handleTranslateNow}
                disabled={translating}
                className="text-xs px-2.5 py-1 rounded-md bg-panel2 text-text hover:bg-border disabled:opacity-50"
              >
                {translating ? "Translating…" : "Translate now"}
              </button>
              <span className="text-xs text-subtext ml-2">
                Not translated yet — jumps ahead of the background queue instead of waiting.
              </span>
              {translateError && <p className="text-xs text-warn mt-1">{translateError}</p>}
            </div>
          )}
          <div className="text-sm text-muted leading-relaxed whitespace-pre-line">{gameSynopsis(game)}</div>
        </div>

        {/* Fixed footer -- never scrolls out of view. */}
        <div className="shrink-0 border-t border-border p-6 pt-4">
          <div className="flex items-center gap-4 text-sm text-subtext mb-4">
            <span className="text-ok">{game.seeds} seeds</span>
            <span className="text-warn">{game.leeches} leeches</span>
            <span>{game.size ?? "Unknown"}</span>
          </div>
          <button
            onClick={onDownload}
            disabled={!hasMagnet || downloading}
            className="w-full bg-accent text-bg font-semibold py-3 rounded-lg disabled:opacity-50 flex items-center justify-center gap-2"
          >
            {downloading ? "Starting…" : hasMagnet ? "Download" : "No magnet link yet"}
          </button>
          {!hasMagnet && (
            <p className="text-xs text-subtext mt-2">
              This result only has a direct-download link (no magnet) — not downloadable from here yet.
            </p>
          )}
          {downloadError && (
            <p className="text-xs text-warn mt-2">Couldn't start the download: {downloadError}</p>
          )}
        </div>
      </div>
    </div>
  );
}
