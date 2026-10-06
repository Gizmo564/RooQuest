import type { Game } from "./tauri";

// The design has no real cover art yet (that's what IGDB metadata is for --
// separate, pending work). Until a game has a real image_url, give it a
// stable gradient placeholder derived from its id, so the grid still reads
// as a game library instead of a blank wall -- same look the design mockup
// used, just deterministic instead of hand-picked.
const PLACEHOLDER_GRADIENTS = [
  "linear-gradient(135deg,#312B52,#1B4B5C)",
  "linear-gradient(135deg,#4A2E4E,#26324F)",
  "linear-gradient(135deg,#532C2C,#4E3A1E)",
  "linear-gradient(135deg,#243A5E,#2B2250)",
  "linear-gradient(135deg,#2E2E36,#1C2B2B)",
  "linear-gradient(135deg,#1F3B3B,#2B4A2B)",
];

export function coverGradient(id: number | null): string {
  const idx = id == null ? 0 : Math.abs(id) % PLACEHOLDER_GRADIENTS.length;
  return PLACEHOLDER_GRADIENTS[idx];
}

export function gameTags(game: Game): string[] {
  if (!game.tags) return [];
  return game.tags
    .split(",")
    .map((t) => t.trim())
    .filter(Boolean);
}

export function gameSynopsis(game: Game): string {
  return game.translated_description || game.description || "No description yet.";
}

// RuTracker titles are scraped as "[VR <Platform>/<Platform>/...] <Name>
// ...", e.g. "[VR Meta Quest 2/Quest Pro/Quest 3/Quest 3S] Zombomber! v0.5
// [...]" -- a leading bracket naming the standalone headset(s) it runs on.
// There's no dedicated platform column, so this parses that bracket prefix
// rather than adding a migration for something derivable on the fly.
//
// NNM-Club titles never carry that leading bracket at all (confirmed
// against real scraped titles -- their only "[VR]" tag trails the title,
// e.g. "... Repack FitGirl [VR]", and just marks the listing as VR-capable,
// not which headset). NNM-Club is a PC-game tracker with no standalone-
// headset releases on it -- every title there is a Windows PC build meant
// to be run through a PC's own GPU with the headset tethered -- so a game
// from that source with no platform bracket is PCVR, not "no platform
// data", and gameSource() below already gives a free, reliable source
// signal for that without needing a second title regex.
const PLATFORM_PREFIX = /^\[VR\s+([^\]]+)\]/i;

export function gamePlatforms(game: Game): string[] {
  const title = game.title ?? "";
  const match = title.match(PLATFORM_PREFIX);
  if (match) {
    return match[1]
      .split("/")
      .map((p) => p.trim())
      .filter(Boolean);
  }
  return gameSource(game) === "NNM-Club" ? ["PCVR"] : [];
}

// Which indexed source a game came from, derived from its post_url's
// domain rather than a dedicated column -- every game has a post_url, and
// the domain is a reliable, free signal for "where did this come from"
// that doesn't need its own migration. Falls back to "Other" for anything
// that doesn't match a known source (shouldn't normally happen).
export function gameSource(game: Game): "RuTracker" | "NNM-Club" | "Other" {
  const url = game.post_url ?? "";
  if (url.includes("nnmclub")) return "NNM-Club";
  if (url.includes("rutracker")) return "RuTracker";
  return "Other";
}

// Best-effort release year, parsed out of the scraped description text
// (format: "<Title>\nДата выпуска: 21 июн 2022 г." in Russian, or
// "<Title>\nRelease date: June 21, 2022" once translated). There's no
// dedicated release-date column, so this is a heuristic over free text --
// good enough for a "sort by year" control, not guaranteed for every row.
export function releaseYear(game: Game): number | null {
  const text = game.translated_description || game.description || "";
  const secondLine = text.split("\n")[1] ?? text;
  const match = secondLine.match(/\b(19|20)\d{2}\b/);
  return match ? parseInt(match[0], 10) : null;
}
