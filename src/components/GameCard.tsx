import type { Game } from "../lib/tauri";
import { coverGradient, gameSource, gameTags } from "../lib/display";

export function GameCard({
  game,
  onOpen,
  onToggleWishlist,
}: {
  game: Game;
  onOpen: () => void;
  onToggleWishlist: () => void;
}) {
  const tags = gameTags(game);
  const source = gameSource(game);
  return (
    <div
      onClick={onOpen}
      className="bg-panel border border-border rounded-2xl overflow-hidden cursor-pointer flex flex-col hover:border-accent/40 transition-colors"
    >
      <div
        className="h-[140px] relative flex items-center justify-center"
        style={game.image_url ? undefined : { background: coverGradient(game.id) }}
      >
        {source !== "Other" && (
          <span className="absolute top-2 left-2 text-[10px] font-medium text-text bg-black/50 rounded px-1.5 py-0.5">
            {source}
          </span>
        )}
        {game.image_url ? (
          <img
            src={game.image_url}
            alt=""
            className="absolute inset-0 w-full h-full object-cover"
          />
        ) : (
          <svg width="34" height="34" viewBox="0 0 24 24" fill="none" className="opacity-35">
            <rect x="3" y="7" width="18" height="11" rx="4" stroke="white" strokeWidth="1.6" />
            <circle cx="8.5" cy="12.5" r="1.6" fill="white" />
            <circle cx="15.5" cy="12.5" r="1.6" fill="white" />
          </svg>
        )}
        <button
          aria-label="Toggle wishlist"
          onClick={(e) => {
            e.stopPropagation();
            onToggleWishlist();
          }}
          className="absolute top-2 right-2 w-7 h-7 rounded-full bg-black/40 flex items-center justify-center"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill={game.wishlist ? "#7C6CF0" : "none"}>
            <path
              d="M12 20.5s-7.5-4.6-10-9.3C.5 7.8 2.4 4.5 6 4c2.2-.3 4.2.9 6 3 1.8-2.1 3.8-3.3 6-3 3.6.5 5.5 3.8 4 7.2-2.5 4.7-10 9.3-10 9.3Z"
              stroke="#E7E6EC"
              strokeWidth="1.5"
              strokeLinejoin="round"
            />
          </svg>
        </button>
      </div>
      <div className="p-3.5 flex flex-col gap-2 flex-1">
        <div className="font-semibold text-sm leading-tight text-text">{game.title ?? "(untitled)"}</div>
        <div className="flex flex-wrap gap-1.5">
          {tags.slice(0, 3).map((tag) => (
            <span key={tag} className="text-[11px] text-muted bg-panel2 rounded px-2 py-0.5">
              {tag}
            </span>
          ))}
        </div>
        <div className="flex items-center gap-3 text-xs text-subtext mt-0.5">
          <span className="flex items-center gap-1 text-ok">{game.seeds} seeds</span>
          <span className="flex items-center gap-1 text-warn">{game.leeches} leeches</span>
          <span className="ml-auto">{game.size ?? "Unknown"}</span>
        </div>
      </div>
    </div>
  );
}
