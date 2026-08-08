import { getSelectedTheme, setTheme, themes, type ThemeName } from "../theme";

export function renderThemeSelector(): string {
  const selected = getSelectedTheme();

  return `
    <label class="settings-field">
      <span>Theme</span>

      <select id="theme-select">
        ${Object.entries(themes)
          .map(
            ([id, theme]) => `
              <option
                value="${id}"
                ${id === selected ? "selected" : ""}
              >
                ${theme.name}
              </option>
            `,
          )
          .join("")}
      </select>
    </label>
  `;
}

export function bindThemeSelector(): void {
  const selector = document.querySelector<HTMLSelectElement>("#theme-select");

  selector?.addEventListener("change", () => {
    setTheme(selector.value as ThemeName);
  });
}
