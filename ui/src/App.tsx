import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { Theme } from "@radix-ui/themes";
import { AuthProvider, useAuth } from "./hooks/useAuth";
import { useTheme } from "./hooks/useTheme";
import { Layout } from "./components/Layout";
import { Login } from "./pages/Login";
import { Search } from "./pages/Search";
import { Player } from "./pages/Player";
import { History } from "./pages/History";
import { Settings } from "./pages/Settings";
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
  toggleTheme,
}: {
  theme: "dark" | "light";
  setTheme: (t: "dark" | "light") => void;
  toggleTheme: () => void;
}) {
  return (
    <Routes>
      <Route path="/login" element={<Login />} />
      <Route
        element={
          <RequireAuth>
            <Layout theme={theme} toggleTheme={toggleTheme} />
          </RequireAuth>
        }
      >
        <Route index element={<Search />} />
        <Route path="player/:id" element={<Player />} />
        <Route path="history" element={<History />} />
        <Route path="settings" element={<Settings theme={theme} setTheme={setTheme} />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}

export function App() {
  const { theme, setTheme, toggleTheme } = useTheme();

  return (
    <Theme appearance={theme} accentColor="blue" radius="medium">
      <BrowserRouter>
        <AuthProvider>
          <AppRoutes theme={theme} setTheme={setTheme} toggleTheme={toggleTheme} />
        </AuthProvider>
      </BrowserRouter>
    </Theme>
  );
}
