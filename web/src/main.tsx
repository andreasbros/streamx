import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@radix-ui/themes/styles.css";
import "video.js/dist/video-js.css";
import "./styles/tokens.css"; // generated from design-tokens/tokens.json
import "./styles/global.css";
import { App } from "./App";
import { registerServiceWorker } from "./lib/register-sw";
import { invalidateCachesOnNewBuild } from "./lib/build-guard";

invalidateCachesOnNewBuild();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);

registerServiceWorker();
