import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api as apiClient } from "../api/client";
import type { ProviderError } from "../api/types";

/**
 * Global provider-health surfaces.
 *
 * Slow: a content request pending longer than 3s shows a top-right
 * pill naming the provider; it disappears the moment the request
 * settles (success or error).
 *
 * Error: a provider failure carried in a response shows a centered,
 * dismissible card with the provider host and the exact error.
 */
export function ProviderHealth() {
  const [slow, setSlow] = useState(false);
  const [error, setError] = useState<ProviderError | null>(null);
  const providerHost = useRef<string>("content provider");
  const slowIds = useRef(new Set<number>());

  useEffect(() => {
    apiClient
      .searchProviders()
      .then((r) => {
        const movie = r.providers.find((p) => p.kind === "movies");
        if (movie) providerHost.current = host(movie.url);
      })
      .catch(() => {});

    const onSlow = (e: Event) => {
      const id = (e as CustomEvent<{ id: number }>).detail?.id;
      if (id === undefined) return;
      slowIds.current.add(id);
      setSlow(true);
      // Hard ceiling: a request that never settles (dropped on
      // navigation, dead connection) cannot pin the pill forever.
      window.setTimeout(() => {
        slowIds.current.delete(id);
        if (slowIds.current.size === 0) setSlow(false);
      }, 60000);
    };
    const onSettled = (e: Event) => {
      const id = (e as CustomEvent<{ id: number }>).detail?.id;
      if (id !== undefined) slowIds.current.delete(id);
      if (slowIds.current.size === 0) setSlow(false);
    };
    const onError = (e: Event) => {
      const detail = (e as CustomEvent<ProviderError>).detail;
      if (detail) setError(detail);
    };
    window.addEventListener("streamx:provider-slow", onSlow);
    window.addEventListener("streamx:provider-settled", onSettled);
    window.addEventListener("streamx:provider-error", onError);
    return () => {
      window.removeEventListener("streamx:provider-slow", onSlow);
      window.removeEventListener("streamx:provider-settled", onSettled);
      window.removeEventListener("streamx:provider-error", onError);
    };
  }, []);

  return (
    <>
      <AnimatePresence>
        {slow && (
          <motion.div
            key="provider-slow"
            initial={{ opacity: 0, y: -12, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -8, scale: 0.98 }}
            transition={{ type: "spring", stiffness: 420, damping: 30 }}
            style={{
              position: "fixed",
              top: 16,
              right: 16,
              zIndex: 1200,
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "10px 16px",
              borderRadius: 14,
              background: "color-mix(in srgb, var(--color-panel-solid, #16181c) 82%, transparent)",
              backdropFilter: "blur(12px)",
              border: "1px solid var(--accent-8, #3b82f6)",
              boxShadow: "0 8px 30px rgba(0,0,0,0.35)",
            }}
          >
            <span style={{ display: "flex", gap: 4 }}>
              {[0, 1, 2].map((i) => (
                <motion.span
                  key={i}
                  animate={{ opacity: [0.25, 1, 0.25] }}
                  transition={{ duration: 0.9, repeat: Infinity, delay: i * 0.18 }}
                  style={{
                    width: 6,
                    height: 6,
                    borderRadius: 999,
                    background: "var(--accent-9, #3b82f6)",
                    display: "inline-block",
                  }}
                />
              ))}
            </span>
            <span style={{ display: "flex", flexDirection: "column", lineHeight: 1.25 }}>
              <span style={{ fontSize: 13, fontWeight: 600 }}>Provider is slow to respond</span>
              <span style={{ fontSize: 12, opacity: 0.7 }}>{providerHost.current}</span>
            </span>
          </motion.div>
        )}
      </AnimatePresence>

      <AnimatePresence>
        {error && (
          <motion.div
            key="provider-error"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            style={{
              position: "fixed",
              inset: 0,
              zIndex: 1300,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: "rgba(0,0,0,0.55)",
              backdropFilter: "blur(4px)",
            }}
            onClick={() => setError(null)}
          >
            <motion.div
              initial={{ opacity: 0, scale: 0.9, y: 16 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.95, y: 8 }}
              transition={{ type: "spring", stiffness: 380, damping: 28 }}
              onClick={(e) => e.stopPropagation()}
              style={{
                width: "min(440px, calc(100vw - 48px))",
                borderRadius: 18,
                padding: 24,
                background: "var(--color-panel-solid, #16181c)",
                border: "1px solid color-mix(in srgb, var(--red-9, #e5484d) 55%, transparent)",
                boxShadow:
                  "0 24px 70px rgba(0,0,0,0.5), 0 0 0 1px rgba(255,255,255,0.04) inset",
                display: "flex",
                flexDirection: "column",
                gap: 14,
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <motion.div
                  animate={{ scale: [1, 1.06, 1] }}
                  transition={{ duration: 2.2, repeat: Infinity }}
                  style={{
                    width: 40,
                    height: 40,
                    borderRadius: 999,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: 20,
                    background:
                      "linear-gradient(135deg, var(--red-9, #e5484d), var(--orange-9, #f76b15))",
                    color: "white",
                    fontWeight: 700,
                  }}
                >
                  !
                </motion.div>
                <div style={{ fontSize: 18, fontWeight: 700 }}>Provider unavailable</div>
              </div>
              <code
                style={{
                  fontSize: 12,
                  padding: "8px 12px",
                  borderRadius: 10,
                  background: "rgba(255,255,255,0.06)",
                  wordBreak: "break-all",
                }}
              >
                {error.url}
              </code>
              <div style={{ fontSize: 14, lineHeight: 1.5 }}>{error.message}</div>
              <div style={{ fontSize: 12.5, opacity: 0.65 }}>
                Titles from this source are hidden until it recovers. Everything else keeps
                working.
              </div>
              <button
                onClick={() => setError(null)}
                style={{
                  alignSelf: "flex-end",
                  padding: "9px 20px",
                  borderRadius: 10,
                  border: "none",
                  cursor: "pointer",
                  fontWeight: 600,
                  fontSize: 14,
                  color: "white",
                  background:
                    "linear-gradient(135deg, var(--accent-9, #3b82f6), var(--accent-10, #2563eb))",
                  boxShadow: "0 4px 18px color-mix(in srgb, var(--accent-9, #3b82f6) 45%, transparent)",
                }}
              >
                Got it
              </button>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}

function host(url: string): string {
  return url.replace(/^https?:\/\//, "").replace(/\/$/, "");
}
