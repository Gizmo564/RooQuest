import { create } from "zustand";

// Tiny cross-component UI flags that don't belong to any one data domain.
// Currently just: is Sideload mid-install, so the sidebar badge (which
// lives outside SideloadTab) can reflect it.
interface UiState {
  sideloadBusy: boolean;
  setSideloadBusy: (busy: boolean) => void;
}

export const useUiStore = create<UiState>((set) => ({
  sideloadBusy: false,
  setSideloadBusy: (busy) => set({ sideloadBusy: busy }),
}));
