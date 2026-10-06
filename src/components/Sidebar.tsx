import { useRef, useState } from "react";

export type Tab = "library" | "downloads" | "sideload" | "settings";

const TABS: { id: Tab; label: string }[] = [
  { id: "library", label: "Library" },
  { id: "downloads", label: "Downloads" },
  { id: "sideload", label: "Sideload" },
  { id: "settings", label: "Settings" },
];

export function Sidebar({
  active,
  onChange,
  activeTorrentCount,
  sideloadInstalling,
}: {
  active: Tab;
  onChange: (t: Tab) => void;
  activeTorrentCount: number;
  sideloadInstalling: boolean;
}) {
  // Easter egg: five clicks on the logo within two seconds takes over the
  // whole window with the Team Rocket logo for a few seconds. Purely
  // cosmetic -- no state is saved, nothing else on the page reacts to it,
  // and it resets itself whenever a click doesn't continue the streak in
  // time. Clicking the overlay (or the timeout) dismisses it early.
  const [showCredit, setShowCredit] = useState(false);
  const [creditLeaving, setCreditLeaving] = useState(false);
  const clickCount = useRef(0);
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dismissTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const leaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Fades/scales the overlay out (team-rocket-overlay-out in globals.css)
  // before actually unmounting it, instead of just vanishing -- a bare
  // show/hide toggle was the "jarring" part.
  const dismissCredit = () => {
    if (dismissTimer.current) clearTimeout(dismissTimer.current);
    if (leaveTimer.current) clearTimeout(leaveTimer.current);
    setCreditLeaving(true);
    leaveTimer.current = setTimeout(() => {
      setShowCredit(false);
      setCreditLeaving(false);
    }, 180);
  };

  const handleLogoClick = () => {
    clickCount.current += 1;
    if (clickTimer.current) clearTimeout(clickTimer.current);
    if (clickCount.current >= 5) {
      clickCount.current = 0;
      setCreditLeaving(false);
      setShowCredit(true);
      if (dismissTimer.current) clearTimeout(dismissTimer.current);
      dismissTimer.current = setTimeout(dismissCredit, 4000);
    } else {
      clickTimer.current = setTimeout(() => {
        clickCount.current = 0;
      }, 2000);
    }
  };

  return (
    <aside className="w-56 shrink-0 bg-panel border-r border-border flex flex-col py-6 px-4">
      <div
        onClick={handleLogoClick}
        className="flex items-center gap-2.5 px-2 pb-7 cursor-default select-none"
        title=" "
      >
        <div className="w-[26px] h-[26px] rounded-lg bg-accent flex items-center justify-center flex-none">
          <KangarooLogo className="w-4 h-4 text-bg" />
        </div>
        <span className="font-semibold text-text text-[19px]">RooQuest</span>
      </div>

      {showCredit && (
        <div
          onClick={dismissCredit}
          className={`team-rocket-overlay fixed inset-0 z-50 flex flex-col items-center justify-center gap-6 bg-bg/95 cursor-default select-none ${
            creditLeaving ? "is-leaving" : ""
          }`}
        >
          <TeamRocketLogo className="team-rocket-logo w-[min(70vw,70vh)] h-[min(70vw,70vh)]" />
          <div className="text-center">
            <div className="text-text font-semibold text-lg mb-1">Prepare for trouble, make it double.</div>
            <div className="text-subtext text-sm">Built with care by Team Rocket.</div>
          </div>
        </div>
      )}

      <nav className="flex flex-col gap-1">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => onChange(t.id)}
            className={`flex items-center gap-2.5 text-left px-3 py-2.5 rounded-lg text-sm font-medium transition-colors ${
              active === t.id ? "bg-panel2 text-text" : "text-subtext hover:text-text"
            }`}
          >
            {t.label}
            {t.id === "downloads" && activeTorrentCount > 0 && (
              <span className="ml-auto bg-accent text-bg text-[11px] font-semibold px-1.5 py-0.5 rounded-full">
                {activeTorrentCount}
              </span>
            )}
            {t.id === "sideload" && sideloadInstalling && (
              <span className="ml-auto w-1.5 h-1.5 rounded-full bg-accent" />
            )}
          </button>
        ))}
      </nav>

      <div className="flex-1" />

      <div className="flex flex-col gap-2.5 px-2.5 py-3 rounded-xl bg-panel2 text-xs text-subtext">
        <div className="flex items-center gap-1.5">
          <span className="w-1.5 h-1.5 rounded-full bg-ok flex-none" />
          <span>
            Engine: <strong className="text-text">rqbit</strong> (primary)
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          <span className="w-1.5 h-1.5 rounded-full bg-ok flex-none" />
          <span>
            qBittorrent fallback: <strong className="text-text">available</strong>
          </span>
        </div>
      </div>
    </aside>
  );
}

function KangarooLogo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" className={className} fill="currentColor">
      <path d="M2 8.53c.02.81.68 1.47 1.5 1.47h1.46v3.79C4.96 15.56 6.4 17 8.18 17h.72c.08 0 .12.09.06.15l-.62.62c-.36.36-.58.85-.57 1.36A1.93 1.93 0 0 0 9.69 21h.56c.42 0 .76-.34.75-.76a.77.77 0 0 0-.77-.74h-.55a.4.4 0 0 1-.38-.26c-.05-.15-.01-.3.09-.4l.8-.8c.25-.25.44-.53.58-.84c.05-.12.17-.2.31-.2h2.047a3.46 3.46 0 0 0 .463 1.96a3.49 3.49 0 0 0 3.11 1.75l.8 1.39l-2.73 1.52c-1.07.6-1.73 1.72-1.73 2.95v1.53c0 .44.32.83.76.89c.39.05.75-.16.9-.51l.58-1.32c.19-.43.5-.66.62-.73l4.1-2.3c.43-.24.73-.63.86-1.1s.07-.96-.18-1.39l-1.17-2.05l2.1-1.21A3.47 3.47 0 0 0 23 16.9v3.581c0 1.92 1.53 3.53 3.45 3.55c1.95.03 3.55-1.55 3.55-3.5v-7.09c0-.24-.2-.44-.44-.44c-.86 0-1.56.7-1.56 1.56v4.35c0 .55-.45 1-1 1s-1-.45-1-1v-5.18c0-4.44-3.52-8.13-7.95-8.23c-3.69-.09-6.43 2.42-7.91 5.55c-.1.21-.26.45-.54.45a.61.61 0 0 1-.6-.6V6h1.01C11.11 6 12 5.11 12 4.01v-.7c0-.17-.14-.31-.31-.31h-1.7C8.89 3 8 3.74 8 4.84c0 .09-.07.16-.16.16H6.02c-.59 0-1.1.41-1.22.99l-.09.41c-.08.35-.38.6-.74.6h-.44C2.69 7 1.98 7.69 2 8.53M7 7.5a.5.5 0 1 1-1 0a.5.5 0 0 1 1 0" />
    </svg>
  );
}

// The Team Rocket "R" logo shown full-window by the sidebar-logo easter egg.
// Kept as its own inline SVG (rather than an <img> asset) so it scales
// crisply to whatever size the overlay gives it, with no extra network/file
// request. Path and viewBox copied verbatim from the provided logo file;
// fill is the logo's own brand red, not currentColor, so it stays red
// regardless of surrounding text color.
function TeamRocketLogo({ className }: { className?: string }) {
  return (
    <svg viewBox="310.69138 82.435944 264.58334 252.94167" className={className}>
      <path
        d="m311.64378 83.370771v251.054959h77.48364v-70.77139h63.9501l37.62975 70.77139h83.39077l-43.60354-83.76129c24.47007-14.33512 41.01971-42.77561 40.9174-78.5611-0.20842-72.90332-53.88388-88.732569-78.66858-88.732569v5.16e-4l-181.09954-5.16e-4zm77.48364 68.044429h77.47537c11.4258-1.6e-4 20.68831 9.26234 20.68815 20.68814-1.3e-4 11.4256-9.26255 20.68779-20.68815 20.68763h-77.47537v-41.37577z"
        fill="#c90000"
      />
    </svg>
  );
}
