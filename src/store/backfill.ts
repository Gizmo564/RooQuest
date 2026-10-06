import { create } from "zustand";
import {
  igdbBackfillAll,
  onIgdbBackfillProgress,
  translateBackfillAll,
  onTranslateBackfillProgress,
  type IgdbBackfillProgress,
  type TranslateBackfillProgress,
} from "../lib/tauri";

// Plain component state would reset every time Settings unmounts (switching
// tabs, say) even though the backend command keeps running regardless --
// which is exactly backwards from "start it and go do something else". A
// module-level Zustand store survives navigation, so the button's
// running/progress state (and the guard against starting a second run on
// top of one already in flight) stay correct no matter which tab is open.
interface BackfillState {
  igdbRunning: boolean;
  igdbProgress: IgdbBackfillProgress | null;
  igdbResult: string | null;
  runIgdb: () => Promise<void>;

  translateRunning: boolean;
  translateProgress: TranslateBackfillProgress | null;
  translateResult: string | null;
  runTranslate: () => Promise<void>;

  subscribed: boolean;
  subscribe: () => void;
}

export const useBackfillStore = create<BackfillState>((set, get) => ({
  igdbRunning: false,
  igdbProgress: null,
  igdbResult: null,
  translateRunning: false,
  translateProgress: null,
  translateResult: null,
  subscribed: false,

  // Called once, from App's top level, so progress events keep updating the
  // store no matter which tab is actually mounted.
  subscribe: () => {
    if (get().subscribed) return;
    set({ subscribed: true });
    onIgdbBackfillProgress((p) => set({ igdbProgress: p }));
    onTranslateBackfillProgress((p) => set({ translateProgress: p }));
  },

  runIgdb: async () => {
    if (get().igdbRunning) return;
    set({ igdbRunning: true, igdbResult: null, igdbProgress: null });
    try {
      const result = await igdbBackfillAll();
      set({ igdbResult: result });
    } catch (err) {
      set({ igdbResult: `Failed: ${err}` });
    } finally {
      set({ igdbRunning: false });
    }
  },

  runTranslate: async () => {
    if (get().translateRunning) return;
    set({ translateRunning: true, translateResult: null, translateProgress: null });
    try {
      const result = await translateBackfillAll();
      set({ translateResult: result });
    } catch (err) {
      set({ translateResult: `Failed: ${err}` });
    } finally {
      set({ translateRunning: false });
    }
  },
}));
