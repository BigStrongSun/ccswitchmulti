import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../../index.css";
import { CodexSessionCollectionStatusFixtureGallery } from "./CodexSessionTrafficPanel.fixture";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Codex session collection fixture root is missing");
}

createRoot(root).render(
  <StrictMode>
    <CodexSessionCollectionStatusFixtureGallery />
  </StrictMode>,
);
