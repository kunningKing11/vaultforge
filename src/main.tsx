import { createRoot } from "react-dom/client";

import { VaultForgeApp } from "./react/App";

import "./react/styles.css";
import "@coinbase/cds-icons/fonts/web/icon-font.css";
import "@coinbase/cds-web/defaultFontStyles";
import "@coinbase/cds-web/globalStyles";
import "@fontsource/ubuntu-mono/400-italic.css";
import "@fontsource/ubuntu-mono/400.css";
import "@fontsource/ubuntu-mono/700-italic.css";
import "@fontsource/ubuntu-mono/700.css";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("App root not found");
}

createRoot(app).render(<VaultForgeApp />);
