import { create } from "zustand";
import { torrentAction, torrentAdd, torrentList, type TorrentInfo } from "../lib/tauri";
import { useSettingsStore } from "./settings";

interface DownloadsState {
  torrents: TorrentInfo[];
  refresh: () => Promise<void>;
  add: (magnet: string, title?: string) => Promise<void>;
  togglePause: (t: TorrentInfo) => Promise<void>;
  remove: (hash: string) => Promise<void>;
}

const isPaused = (state: string) => state.toLowerCase().includes("pause");

export const useDownloadsStore = create<DownloadsState>((set, get) => ({
  torrents: [],

  refresh: async () => {
    try {
      const torrents = await torrentList();
      set({ torrents });
    } catch {
      // Downloads tab polls this regularly; a transient failure shouldn't
      // wipe what's currently shown.
    }
  },

  add: async (magnet, _title) => {
    const savepath = useSettingsStore.getState().settings?.downloadPath ?? ".";
    await torrentAdd(magnet, savepath);
    await get().refresh();
  },

  togglePause: async (t) => {
    await torrentAction(t.hash, isPaused(t.state) ? "resume" : "pause");
    await get().refresh();
  },

  remove: async (hash) => {
    await torrentAction(hash, "delete");
    set((s) => ({ torrents: s.torrents.filter((t) => t.hash !== hash) }));
  },
}));

export { isPaused };
