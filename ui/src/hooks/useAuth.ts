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

interface AuthState {
  user: User | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  login: (username: string, password: string) => Promise<void>;
  register: (username: string, password: string) => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthState | null>(null);

const PUBLIC_ROUTES = ["/login"];

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const navigate = useNavigate();
  const location = useLocation();

  const checkAuth = useCallback(async () => {
    const token = getToken();
    if (!token || isTokenExpired(token)) {
      removeToken();
      setUser(null);
      setIsLoading(false);
      return;
    }

    try {
      const userData = await api.me();
      setUser(userData);
    } catch (err) {
      if (err instanceof ApiRequestError && err.status === 401) {
        removeToken();
        setUser(null);
      }
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    checkAuth();
  }, [checkAuth]);

  useEffect(() => {
    if (!isLoading && !user && !PUBLIC_ROUTES.includes(location.pathname)) {
      navigate("/login", { replace: true });
    }
  }, [isLoading, user, location.pathname, navigate]);

  const login = useCallback(
    async (username: string, password: string) => {
      const response = await api.login({ username, password });
      setToken(response.token);
      const userData = await api.me();
      setUser(userData);
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
      navigate("/", { replace: true });
    },
    [navigate]
  );

  const logout = useCallback(() => {
    removeToken();
    setUser(null);
    navigate("/login", { replace: true });
  }, [navigate]);

  const value: AuthState = {
    user,
    isAuthenticated: !!user,
    isLoading,
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
