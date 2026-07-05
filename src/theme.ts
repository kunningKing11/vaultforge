import theme from "./theme.json";

type ThemeValue = string | number | ThemeValues;
type ThemeValues = { [key: string]: ThemeValue };

function toKebabCase(value: string): string {
  return value.replace(/[A-Z]/g, (match) => `-${match.toLowerCase()}`);
}

function hexToRgbChannels(value: string): string | null {
  const normalized = value.trim().replace(/^#/, "");
  const hex = normalized.length === 3
    ? normalized.split("").map((char) => `${char}${char}`).join("")
    : normalized;

  if (!/^[0-9a-fA-F]{6}$/.test(hex)) return null;

  const numberValue = Number.parseInt(hex, 16);
  return `${(numberValue >> 16) & 255} ${(numberValue >> 8) & 255} ${numberValue & 255}`;
}

function applyThemeSection(prefix: string, values: ThemeValues, root: HTMLElement): void {
  Object.entries(values).forEach(([key, value]) => {
    const property = `${prefix}-${toKebabCase(key)}`;

    if (typeof value === "string" || typeof value === "number") {
      const stringValue = String(value);
      root.style.setProperty(`--${property}`, stringValue);

      if (prefix === "color") {
        const rgb = hexToRgbChannels(stringValue);
        if (rgb) root.style.setProperty(`--${property}-rgb`, rgb);
      }
      return;
    }

    applyThemeSection(property, value, root);
  });
}

export function applyTheme(): void {
  const root = document.documentElement;
  applyThemeSection("font", theme.fonts, root);
  applyThemeSection("color", theme.colors, root);
  applyThemeSection("opacity", theme.opacity, root);
  applyThemeSection("shadow", theme.shadows, root);
}

export { theme };
