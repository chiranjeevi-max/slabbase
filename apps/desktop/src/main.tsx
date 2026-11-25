import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyAppearance, initialAppearance } from "./appearance";
import { isTauri } from "./native";
import "./styles/tokens.css";
import "./styles.css";

if (isTauri()) {
  document.documentElement.dataset["native"] = "true";
}
applyAppearance(initialAppearance);

const container = document.getElementById("root");
if (!container) {
  throw new Error("missing #root element");
}

createRoot(container).render(<App />);
