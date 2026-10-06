import { create } from "zustand";
import { deleteGame, getGames, setWishlist, type Game } from "../lib/tauri";

interface LibraryState {
  games: Game[];
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  toggleWishlist: (id: number, wishlist: boolean) => Promise<void>;
  remove: (id: number) => Promise<void>;
  // Replaces one game's row in-place (e.g. after a one-off "Translate now"
  // or IGDB match) without refetching the whole library.
  patchGame: (game: Game) => void;
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  games: [],
  loading: false,
  error: null,

  load: async () => {
    set({ loading: true, error: null });
    try {
      const games = await getGames();
      set({ games, loading: false });
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  toggleWishlist: async (id, wishlist) => {
    await setWishlist(id, wishlist);
    set((s) => ({
      games: s.games.map((g) => (g.id === id ? { ...g, wishlist: wishlist ? 1 : 0 } : g)),
    }));
  },

  remove: async (id) => {
    await deleteGame(id);
    set((s) => ({ games: s.games.filter((g) => g.id !== id) }));
  },

  patchGame: (game) => {
    set((s) => ({ games: s.games.map((g) => (g.id === game.id ? game : g)) }));
  },
}));
