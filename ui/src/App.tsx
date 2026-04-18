import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { Theme } from "@radix-ui/themes";
import { AuthProvider, useAuth } from "./hooks/useAuth";
import { FavouritesProvider } from "./hooks/useFavourites";
import { AudioPlayerProvider } from "./hooks/useAudioPlayer";
import { useTheme } from "./hooks/useTheme";
import { Layout } from "./components/Layout";
import { Login } from "./pages/Login";
import { Search } from "./pages/Search";
import { Player } from "./pages/Player";
import { History } from "./pages/History";
import { Browse } from "./pages/Browse";
import { Movie } from "./pages/Movie";
import { Favourites } from "./pages/Favourites";
import { TvSearch } from "./pages/TvSearch";
import { TvShow } from "./pages/TvShow";
import { MusicSearch } from "./pages/MusicSearch";
import { Settings } from "./pages/Settings";
import { SurroundSound } from "./pages/SurroundSound";
import { Admin } from "./pages/Admin";
import { MusicPlayer } from "./pages/MusicPlayer";
import type { ReactNode } from "react";

function RequireAuth({ children }: { children: ReactNode }) {
  const { isAuthenticated, isLoading } = useAuth();

  if (isLoading) return null;
  if (!isAuthenticated) return <Navigate to="/login" replace />;

  return <>{children}</>;
}

function AppRoutes({
  theme,
  setTheme,
}: {
  theme: "dark" | "light";
  setTheme: (t: "dark" | "light") => void;
}) {
  return (
    <>
    <Routes>
      <Route path="/login" element={<Login />} />
      <Route
        element={
          <RequireAuth>
            <FavouritesProvider>
              <AudioPlayerProvider>
                <Layout />
              </AudioPlayerProvider>
            </FavouritesProvider>
          </RequireAuth>
        }
      >
        <Route index element={<Search />} />
        <Route path="browse/:category" element={<Browse />} />
        <Route path="movie" element={<Movie />} />
        <Route path="player/:id" element={<Player />} />
        <Route path="tv" element={<TvSearch />} />
        <Route path="tv/show" element={<TvShow />} />
        <Route path="music" element={<MusicSearch />} />
        <Route path="music/play/:streamId/:fileIndex" element={<MusicPlayer />} />
        <Route path="history" element={<History />} />
        <Route path="surround" element={<SurroundSound />} />
        <Route path="favourites" element={<Favourites />} />
        <Route path="settings" element={<Settings theme={theme} setTheme={setTheme} />} />
        <Route path="admin" element={<Admin />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
    </>
  );
}

export function App() {
  const { theme, setTheme } = useTheme();

  return (
    <Theme appearance={theme} accentColor="blue" radius="medium">
      <BrowserRouter>
        <AuthProvider>
          <AppRoutes theme={theme} setTheme={setTheme} />
        </AuthProvider>
      </BrowserRouter>
    </Theme>
  );
}
