import "./styles.css";
import { boot } from "./events";

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("App root not found");
}

export const appRoot = app;

void boot();
