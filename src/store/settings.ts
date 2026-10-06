import { create } from "zustand";
import { getSettings, saveSettings, type Settings } from "../lib/tauri";

interface SettingsState {
  settings: Settings | null;
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  update: (partial: Partial<Settings>) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  settings: null,
  loading: false,
  error: null,

  load: async () => {
    set({ loading: true, error: null });
    try {
      const settings = await getSettings();
      set({ settings, loading: false });
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  update: async (partial) => {
    const settings = await saveSettings(partial);
    set({ settings });
  },
}));
