# RooQuest

A native macOS desktop app for indexing and managing VR content listed on public torrent-tracker forums (RuTracker and NNM-Club), with built-in tools to sideload the results onto a Meta Quest headset over ADB.

RooQuest is a ground-up Rust/Tauri rewrite of the original [VRSideForge](https://github.com/yGuilhermy/VRSideForge) project — same idea, new engine: a native Rust backend instead of a Node.js server, a single packaged desktop app instead of a browser + server combo, and a handful of quality-of-life features (auto-scraping, headset storage visualization, per-source filtering, IGDB metadata enrichment) that the original didn't have.

> Built with care by Team Rocket. 🚀

---

## ⚠️ Important: what this tool is, and isn't

**RooQuest is an indexing and bookkeeping tool. It does not host, mirror, or distribute any copyrighted content itself.**

Everything RooQuest shows you comes from scraping public listing pages on third-party forums (RuTracker, NNM-Club). RooQuest parses those pages into a searchable local library and — if you choose to download something — hands the resulting magnet link to your own torrent client, the same way clicking that link in a browser would.

**Using RooQuest, or any tool like it, to download or install games and other software you do not own a legitimate license for is copyright infringement. It is illegal in most jurisdictions, and it is a direct violation of the Terms of Service of the sites RooQuest indexes.** Neither RooQuest nor the people who built it are responsible for what you choose to download with it, or for any consequences — legal, account-related, or otherwise — that result from that choice.

If you don't already own a legal license to a piece of content, downloading it through RooQuest (or through any other means) is not something this project condones, supports, or was built to enable. Use it responsibly, and at your own risk.

---

## What it does

- **Scrapes RuTracker and NNM-Club** for VR-tagged listings and builds a local, searchable library of what's posted there — titles, platform tags, cover art and metadata (via IGDB), and file size.
  - RuTracker coverage spans both its active torrent listing and its full topic archive, plus a few keyword-narrowed searches, rather than a single capped query — the active-listing view alone only ever exposes a few hundred results.
  - NNM-Club has no dedicated VR forum, so RooQuest searches across the PC-game genre forums that actually carry VR-tagged ("[VR]") titles and combines the results.
- **Auto-scrape, or scrape on demand.** Turn on an auto-scrape toggle per source in Settings and RooQuest will quietly re-check that source on its own every few hours (pausing while you're actively using the Mac), or just hit "Scrape now" whenever you want a fresh pull. A source only ever has one scrape running at a time — a manual click while auto-scrape is already running just reports back that one's already in progress rather than starting a second, competing one.
- **Resolves listings to a working download link.** For listings that only expose a raw `.torrent` file rather than a ready-made magnet link, RooQuest fetches and parses the `.torrent` file itself (no third-party torrent-file parser) and derives the magnet link from it, so the download button works the same way regardless of which format a given forum post used.
- **IGDB metadata enrichment.** Matches scraped titles against IGDB for a real description, genre, developer/publisher, and cover art, with a one-click "scrape IGDB now" backfill in Settings to retry anything that didn't match the first time.
- **Sideloads straight to your Quest.** Connect a Meta Quest over USB (with ADB debugging enabled) and install downloaded `.apk`/OBB packages directly from the app, back up or uninstall existing apps, and see a live bar showing how much storage is used and free on the headset before you push anything to it. A game only shows as ready to install once its download has actually finished — a partially-downloaded folder shows its live progress instead.
- **Local library management.** Filter your library by platform (including PCVR, for NNM-Club's Windows-tethered titles) or by source (RuTracker vs. NNM-Club), sort it, and manage local downloads and backups from the Downloads tab.
- **Offline mode**, translation of listing titles to your preferred language, and login via a real in-app browser window that reuses your normal session cookie for scraping, all configurable in Settings.

## Getting started

1. **Install RooQuest** (see [Installation](#installation) below).
2. **Open Settings** and configure the sources you want:
   - For RuTracker and/or NNM-Club, log in through the in-app login window (RooQuest opens a real browser window pointed at the site so you can sign in normally; it just reuses the resulting session cookie for scraping).
   - Optionally turn on **auto-scrape** for either source so RooQuest keeps your library fresh without you having to remember to check back.
   - Optionally fill in IGDB API credentials for richer cover art and metadata, and a target language if you want listing titles translated.
3. **Scrape.** Either wait for auto-scrape to run, or go to the source panel and hit "Scrape RuTracker now" / "Scrape NNM-Club now" for an immediate pull.
4. **Browse your Library.** Use the Platform and Source filters to narrow things down, and hit download on anything you want to grab — RooQuest will resolve it to a magnet link and hand it to your system's default torrent client (rqbit by default, with qBittorrent as a fallback).
5. **Sideload to your headset.** Plug your Quest in over USB with developer mode and ADB debugging enabled, open the Sideload tab, and RooQuest will list connected devices, show headset storage at a glance, and let you install, back up, or uninstall apps directly.

## Installation

### macOS

1. Download the latest `RooQuest.dmg` (or `.pkg`) from the [Releases](../../releases) page of this repository.
2. Open the downloaded file and drag RooQuest into your Applications folder (for the `.dmg`) or run the installer (for the `.pkg`).
3. **This build is unsigned** (no Apple Developer ID), so the first time you open it, macOS Gatekeeper will likely block it with a message like "RooQuest can't be opened because it is from an unidentified developer." To get past this:
   - Right-click (or Control-click) the RooQuest app in Applications and choose **Open**, then confirm **Open** in the dialog that appears — you only need to do this once; or
   - Run the following in Terminal, then open the app normally:
     ```
     xattr -cr /Applications/RooQuest.app
     ```
4. On first launch, RooQuest will create its own data folder (database, downloaded files, backups) under your Documents folder.

## Building from source

RooQuest is built with [Tauri 2](https://tauri.app/) (Rust backend + React/TypeScript frontend).

```bash
cd desktop
npm install
cargo tauri dev     # run in development
cargo tauri build   # produce a release build / installer for your platform
```

Requires a working Rust toolchain and Node.js. See the [Tauri prerequisites guide](https://tauri.app/start/prerequisites/) for platform-specific setup (Xcode command line tools on macOS, etc.).

## Relationship to VRSideForge

RooQuest grew out of [VRSideForge](https://github.com/yGuilhermy/VRSideForge) and its fork at [Gizmo564/VRSideForge](https://github.com/Gizmo564/VRSideForge), which remains the original Node.js/web implementation and continues to exist as its own project, untouched by this rewrite. RooQuest lives in its own repository as a from-scratch Rust/Tauri desktop client with no shared codebase, built by the same contributor as a separate, independently packaged app.

## Credits

Built with care by **Team Rocket**. 🚀 (Look for us in the sidebar — five quick clicks on the logo and we'll say hi.)
