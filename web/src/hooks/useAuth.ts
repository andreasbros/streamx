import {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  type ReactNode,
} from "react";
import { createElement } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import { api, ApiRequestError } from "../api/client";
import { getToken, setToken, removeToken, isTokenExpired } from "../lib/auth";
import type { User } from "../api/types";
import { debugLog } from "../lib/debug-log";

interface AuthState {
  user: User | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  isGuest: boolean;
  guestStreamId: string | null;
  login: (username: string, password: string) => Promise<void>;
  register: (username: string, password: string) => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthState | null>(null);

const PUBLIC_ROUTES = ["/login"];

function decodeJwtPayload(token: string): Record<string, unknown> | null {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;
    const payload = atob(parts[1]!.replace(/-/g, "+").replace(/_/g, "/"));
    return JSON.parse(payload);
  } catch {
    return null;
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isGuest, setIsGuest] = useState(false);
  const [guestStreamId, setGuestStreamId] = useState<string | null>(null);
  const navigate = useNavigate();
  const location = useLocation();

  const checkAuth = useCallback(async () => {
    // Check for existing valid non-guest session first
    const existingToken = getToken();
    const existingPayload = existingToken ? decodeJwtPayload(existingToken) : null;
    const hasValidSession = existingToken && !isTokenExpired(existingToken) && existingPayload?.role !== "guest";

    // Only use guest token from URL if user is NOT already logged in
    const params = new URLSearchParams(location.search);
    const guestToken = params.get("guest");
    if (guestToken && !hasValidSession && !isTokenExpired(guestToken)) {
      const payload = decodeJwtPayload(guestToken);
      if (payload?.role === "guest" && payload?.stream_id) {
        debugLog.info("auth", `Guest token for stream ${payload.stream_id}`);
        setToken(guestToken);
        setUser({ id: "guest", username: "guest", is_admin: false, created_at: "" });
        setIsGuest(true);
        setGuestStreamId(payload.stream_id as string);
        setIsLoading(false);
        return;
      }
    }

    const token = existingToken;
    if (!token || isTokenExpired(token)) {
      removeToken();
      setUser(null);
      setIsGuest(false);
      setGuestStreamId(null);
      setIsLoading(false);
      return;
    }

    // Check if stored token is a guest token
    const payload = decodeJwtPayload(token);
    if (payload?.role === "guest") {
      setUser({ id: "guest", username: "guest", is_admin: false, created_at: "" });
      setIsGuest(true);
      setGuestStreamId((payload.stream_id as string) || null);
      setIsLoading(false);
      return;
    }

    try {
      const userData = await api.me();
      setUser(userData);
      setIsGuest(false);
      setGuestStreamId(null);
    } catch (err) {
      if (err instanceof ApiRequestError && err.status === 401) {
        removeToken();
        setUser(null);
      }
    } finally {
      setIsLoading(false);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    checkAuth();
  }, [checkAuth]);

  useEffect(() => {
    if (isLoading) return;
    if (user) {
      // Guest can only access player and music play pages
      if (isGuest && !location.pathname.startsWith("/player/") && !location.pathname.startsWith("/music/play/")) {
        navigate("/login", { replace: true });
      }
      return;
    }
    if (!PUBLIC_ROUTES.includes(location.pathname) && !location.pathname.startsWith("/player/") && !location.pathname.startsWith("/music/play/")) {
      navigate("/login", { replace: true });
    }
  }, [isLoading, user, isGuest, location.pathname, navigate]);

  const login = useCallback(
    async (username: string, password: string) => {
      const response = await api.login({ username, password });
      setToken(response.token);
      const userData = await api.me();
      setUser(userData);
      setIsGuest(false);
      setGuestStreamId(null);
      navigate("/", { replace: true });
    },
    [navigate]
  );

  const register = useCallback(
    async (username: string, password: string) => {
      const response = await api.register({ username, password });
      setToken(response.token);
      const userData = await api.me();
      setUser(userData);
      setIsGuest(false);
      setGuestStreamId(null);
      navigate("/", { replace: true });
    },
    [navigate]
  );

  const logout = useCallback(() => {
    removeToken();
    setUser(null);
    setIsGuest(false);
    setGuestStreamId(null);
    navigate("/login", { replace: true });
  }, [navigate]);

  const value: AuthState = {
    user,
    isAuthenticated: !!user,
    isLoading,
    isGuest,
    guestStreamId,
    login,
    register,
    logout,
  };

  return createElement(AuthContext.Provider, { value }, children);
}

export function useAuth(): AuthState {
  const context = useContext(AuthContext);
  if (!context) {
    throw new Error("useAuth must be used within an AuthProvider");
  }
  return context;
}
