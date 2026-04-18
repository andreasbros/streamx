import { debugLog } from "./debug-log";

export function registerServiceWorker(): void {
  if (!("serviceWorker" in navigator)) {
    debugLog.info("sw", "Service workers not supported");
    return;
  }
  const swMeta = document.querySelector<HTMLMetaElement>('meta[name="sw-url"]');
  const swUrl = swMeta?.content ?? "/sw.js";
  window.addEventListener("load", () => {
    navigator.serviceWorker.register(swUrl, { scope: "/" }).then(
      (reg) => debugLog.info("sw", `Registered, scope: ${reg.scope}`),
      (err) => debugLog.error("sw", `Registration failed: ${err}`),
    );
  });
}
