/** Wipe transient client caches when the server build changes.
 *
 * The build hash is read from this bundle's own script URL
 * (`/assets/{hash}/index-*.js`, rewritten server-side). On a hash
 * change every sessionStorage cache (browse rows, search state) is
 * cleared so a new release can never render stale data. localStorage
 * is left alone: it holds the auth token and user preferences, and
 * user data (favourites, playlists, music, history) lives server-side.
 */
export function invalidateCachesOnNewBuild() {
  try {
    const script = document.querySelector<HTMLScriptElement>('script[src*="/assets/"]');
    const hash = script?.src.match(/\/assets\/([0-9a-f]{6,16})\//)?.[1];
    if (!hash) return; // dev server: unversioned asset paths
    const KEY = "streamx_build_hash";
    if (localStorage.getItem(KEY) !== hash) {
      sessionStorage.clear();
      localStorage.setItem(KEY, hash);
    }
  } catch {
    // Storage unavailable (private mode restrictions); nothing to do.
  }
}
