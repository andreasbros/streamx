import {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  type ReactNode,
} from "react";
import { createElement } from "react";
import { api } from "../api/client";
import type { FavouriteItem } from "../api/types";

interface FavouritesState {
  favourites: FavouriteItem[];
  isLoading: boolean;
  isFavourite: (title: string, year?: number | null) => boolean;
  addFavourite: (item: Omit<FavouriteItem, "id" | "user_id" | "created_at">) => Promise<void>;
  removeFavourite: (id: string) => Promise<void>;
  removeFavouriteByTitle: (title: string, year?: number | null) => Promise<void>;
  refresh: () => Promise<void>;
}

const FavouritesContext = createContext<FavouritesState | null>(null);

export function FavouritesProvider({ children }: { children: ReactNode }) {
  const [favourites, setFavourites] = useState<FavouriteItem[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      const res = await api.getFavourites();
      setFavourites(res.items);
    } catch {
      // ignore
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const isFavourite = useCallback(
    (title: string, year?: number | null) => {
      return favourites.some(
        (f) => f.title === title && (year == null || f.year === year)
      );
    },
    [favourites]
  );

  const addFavourite = useCallback(
    async (item: Omit<FavouriteItem, "id" | "user_id" | "created_at">) => {
      const res = await api.addFavourite(item);
      setFavourites((prev) => [res, ...prev]);
    },
    []
  );

  const removeFavourite = useCallback(async (id: string) => {
    await api.deleteFavourite(id);
    setFavourites((prev) => prev.filter((f) => f.id !== id));
  }, []);

  const removeFavouriteByTitle = useCallback(
    async (title: string, year?: number | null) => {
      const match = favourites.find(
        (f) => f.title === title && (year == null || f.year === year)
      );
      if (match) {
        await removeFavourite(match.id);
      }
    },
    [favourites, removeFavourite]
  );

  return createElement(
    FavouritesContext.Provider,
    {
      value: {
        favourites,
        isLoading,
        isFavourite,
        addFavourite,
        removeFavourite,
        removeFavouriteByTitle,
        refresh,
      },
    },
    children
  );
}

export function useFavourites(): FavouritesState {
  const ctx = useContext(FavouritesContext);
  if (!ctx) {
    throw new Error("useFavourites must be used within FavouritesProvider");
  }
  return ctx;
}
