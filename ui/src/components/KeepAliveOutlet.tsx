import { useRef, useEffect } from "react";
import { useLocation, useNavigationType, useOutlet } from "react-router-dom";

interface CachedRoute {
  pathname: string;
  element: React.ReactNode;
  scrollY: number;
}

const MAX_CACHED = 5;

export function KeepAliveOutlet() {
  const location = useLocation();
  const navigationType = useNavigationType();
  const outlet = useOutlet();
  const cacheRef = useRef<CachedRoute[]>([]);
  const prevPathRef = useRef(location.pathname);

  // On route change: save scroll of old route, restore scroll of new route
  useEffect(() => {
    const prevPath = prevPathRef.current;
    const newPath = location.pathname;

    if (prevPath !== newPath) {
      // Save current scroll for the route we're leaving
      const leaving = cacheRef.current.find((r) => r.pathname === prevPath);
      if (leaving) {
        leaving.scrollY = window.scrollY;
      }
      prevPathRef.current = newPath;
    }

    // Only restore scroll on back/forward navigation (POP)
    // For forward navigation (PUSH/REPLACE), always start at top
    if (navigationType === "POP") {
      const entering = cacheRef.current.find((r) => r.pathname === newPath);
      if (entering && entering.scrollY > 0) {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            window.scrollTo(0, entering.scrollY);
          });
        });
      } else {
        window.scrollTo(0, 0);
      }
    } else {
      window.scrollTo(0, 0);
    }
  }, [location.pathname, navigationType]);

  // Update cache
  const existing = cacheRef.current.find((r) => r.pathname === location.pathname);
  if (existing) {
    existing.element = outlet;
  } else {
    cacheRef.current.push({ pathname: location.pathname, element: outlet, scrollY: 0 });
    if (cacheRef.current.length > MAX_CACHED) {
      const idx = cacheRef.current.findIndex((r) => r.pathname !== location.pathname);
      if (idx >= 0) cacheRef.current.splice(idx, 1);
    }
  }

  return (
    <>
      {cacheRef.current.map((route) => (
        <div
          key={route.pathname}
          style={{
            display: route.pathname === location.pathname ? "block" : "none",
          }}
        >
          {route.element}
        </div>
      ))}
    </>
  );
}
