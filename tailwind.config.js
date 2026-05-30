/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        display: ["Inter", "ui-sans-serif", "system-ui", "sans-serif"],
      },
      colors: {
        ink: "#071013",
        cobalt: "#3b82f6",
        acid: "#b8ff5c",
        violet: "#8b5cf6",
      },
      boxShadow: {
        glow: "0 0 60px rgba(184, 255, 92, 0.16)",
      },
    },
  },
  plugins: [],
};
