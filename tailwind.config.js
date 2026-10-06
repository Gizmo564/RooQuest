/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // Carried over from the vanilla stopgap UI's dark palette so the
        // visual identity doesn't jump when this migration lands.
        bg: "#0E0F14",
        panel: "#15141C",
        panel2: "#1A1924",
        border: "#1F1E28",
        text: "#E7E6EC",
        subtext: "#83808D",
        muted: "#9A98A3",
        accent: "#7C6CF0",
        ok: "#4ADE80",
        warn: "#F87171",
        blocked: "#F0B429",
      },
    },
  },
  plugins: [],
};
