/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        display: ["var(--font-display)", "ui-sans-serif", "system-ui", "sans-serif"],
      },
      colors: {
        ink: "rgb(var(--color-ink-rgb) / <alpha-value>)",
        cobalt: "rgb(var(--color-cobalt-rgb) / <alpha-value>)",
        acid: "rgb(var(--color-accent-rgb) / <alpha-value>)",
        violet: "rgb(var(--color-violet-rgb) / <alpha-value>)",
        error: "rgb(var(--color-error-rgb) / <alpha-value>)",
        warning: "rgb(var(--color-warning-rgb) / <alpha-value>)",
        ok: "rgb(var(--color-ok-rgb) / <alpha-value>)",
      },
      boxShadow: {
        glow: "var(--shadow-glow)",
      },
    },
  },
  plugins: [],
};
