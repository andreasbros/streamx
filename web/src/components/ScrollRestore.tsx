import { useEffect, useRef } from "react";
import { useLocation } from "react-router-dom";

const scrollPositions = new Map<string, number>();

export function ScrollRestore() {
  const { pathname } = useLocation();
  const prevPath = useRef(pathname);

  useEffect(() => {
    // Save scroll position of the page we're leaving
    if (prevPath.current !== pathname) {
      scrollPositions.set(prevPath.current, window.scrollY);
      prevPath.current = pathname;
    }

    // Restore scroll position for the page we're entering
    const saved = scrollPositions.get(pathname);
    if (saved !== undefined) {
      // Delay to let the DOM render first
      requestAnimationFrame(() => {
        window.scrollTo(0, saved);
      });
    } else {
      window.scrollTo(0, 0);
    }
  }, [pathname]);

  return null;
}
