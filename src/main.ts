import { boot } from "./events";
import { applyTheme } from "./theme";

import "./styles.css";
import "@fontsource-variable/manrope/wght.css";
import "@fontsource-variable/tektur/wght.css";
import "@fontsource/ubuntu-mono/latin-400.css";
import "@fontsource/ubuntu-mono/latin-700.css";

applyTheme();

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("App root not found");
}

export const appRoot = app;

void boot();
