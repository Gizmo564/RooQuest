// Per-source (RuTracker/NNM-Club/Server) login + check-login status, shared
// between the Settings tab's torrent-source rows and anything else that
// cares whether a source is authenticated (e.g. disabling a scrape button).
import { create } from "zustand";
import { checkSourceLogin, openSourceLogin, type SourceLabel } from "../lib/tauri";

export type SourceStatus = "unknown" | "opening" | "checking" | "logged_in" | "logged_out" | "error" | "blocked";

interface SourceState {
  status: Record<SourceLabel, SourceStatus>;
  message: Record<SourceLabel, string>;
  login: (source: SourceLabel) => Promise<void>;
  check: (source: SourceLabel) => Promise<void>;
}

export const useSourcesStore = create<SourceState>((set, get) => ({
  status: { main: "unknown", nnmclub: "unknown", server: "blocked" },
  message: {
    main: "",
    nnmclub: "",
    server: "blocked — needs API key",
  },

  login: async (source) => {
    set((s) => ({ status: { ...s.status, [source]: "opening" } }));
    try {
      await openSourceLogin(source);
      set((s) => ({
        status: { ...s.status, [source]: "unknown" },
        message: { ...s.message, [source]: "window opened — log in there" },
      }));
    } catch (err) {
      set((s) => ({
        status: { ...s.status, [source]: "error" },
        message: { ...s.message, [source]: `failed to open: ${err}` },
      }));
    }
  },

  check: async (source) => {
    set((s) => ({ status: { ...s.status, [source]: "checking" } }));
    try {
      const loggedIn = await checkSourceLogin(source);
      set((s) => ({
        status: { ...s.status, [source]: loggedIn ? "logged_in" : "logged_out" },
        message: { ...s.message, [source]: loggedIn ? "logged in" : "not logged in" },
      }));
    } catch (err) {
      set((s) => ({
        status: { ...s.status, [source]: "error" },
        message: { ...s.message, [source]: `check failed: ${err}` },
      }));
    }
  },
}));
