import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@radix-ui/themes/styles.css";
import "video.js/dist/video-js.css";
import "./styles/global.css";
import { App } from "./App";
import { registerServiceWorker } from "./lib/register-sw";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);

registerServiceWorker();
