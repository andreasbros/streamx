import { useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Flex,
  Text,
  Skeleton,
  Tabs,
} from "@radix-ui/themes";
import { PlayIcon, Cross2Icon } from "@radix-ui/react-icons";
import { useFavourites } from "../hooks/useFavourites";
import type { FavouriteItem, SearchResultGroup } from "../api/types";

function restoreGroup(fav: FavouriteItem): SearchResultGroup | null {
  try {
    const meta = fav.metadata_json ? JSON.parse(fav.metadata_json) : {};
    return {
      title: fav.title,
      year: fav.year ?? undefined,
      rating: fav.rating ?? undefined,
      genres: meta.genres ?? [],
      summary: meta.summary,
      imdb_code: meta.imdb_code,
      trailer_code: meta.trailer_code,
      poster: fav.poster_url ?? undefined,
      poster_small: meta.poster_small,
      poster_medium: meta.poster_medium ?? fav.poster_url ?? undefined,
      poster_large: meta.poster_large,
      backdrop: meta.backdrop,
      variants: meta.variants ?? [],
    };
  } catch {
    return null;
  }
}

function FavTile({
  fav,
  onRemove,
}: {
  fav: FavouriteItem;
  onRemove: (id: string) => void;
}) {
  const navigate = useNavigate();
  const [imgError, setImgError] = useState(false);

  const handleClick = () => {
    // Stream favourite: open the player directly
    if (fav.info_hash) {
      const group = restoreGroup(fav);
      navigate(`/player/${fav.info_hash}`, {
        state: {
          poster: fav.poster_url,
          meta: group ? {
            title: group.title,
            year: group.year,
            rating: group.rating,
            genres: group.genres,
            summary: group.summary,
            imdb_code: group.imdb_code,
            trailer_code: group.trailer_code,
            poster_large: group.poster_large,
            backdrop: group.backdrop,
          } : undefined,
        },
      });
      return;
    }
    // Movie title favourite: open the movie page
    const group = restoreGroup(fav);
    if (group) {
      navigate("/movie", { state: group });
    }
  };

  return (
    <div onClick={handleClick} style={{ cursor: "pointer", position: "relative" }}>
      {fav.poster_url && !imgError ? (
        <img
          src={fav.poster_url}
          alt=""
          loading="lazy"
          onError={() => setImgError(true)}
          style={{
            borderRadius: 6,
            objectFit: "cover",
            width: "100%",
            aspectRatio: "2/3",
            background: "var(--gray-a3)",
            display: "block",
          }}
        />
      ) : (
        <Flex
          align="center"
          justify="center"
          style={{
            width: "100%",
            aspectRatio: "2/3",
            borderRadius: 6,
            background: "var(--gray-a3)",
          }}
        >
          <PlayIcon width={24} height={24} />
        </Flex>
      )}

      {/* Remove button */}
      <div
        onClick={(e) => {
          e.stopPropagation();
          onRemove(fav.id);
        }}
        style={{
          position: "absolute",
          top: 6,
          right: 6,
          width: 24,
          height: 24,
          borderRadius: "50%",
          background: "rgba(0, 0, 0, 0.6)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          cursor: "pointer",
        }}
      >
        <Cross2Icon width={12} height={12} color="white" />
      </div>

      <Text
        size="1"
        weight="medium"
        style={{
          display: "-webkit-box",
          WebkitLineClamp: 2,
          WebkitBoxOrient: "vertical",
          overflow: "hidden",
          marginTop: 4,
          lineHeight: 1.3,
        }}
      >
        {fav.title}
      </Text>
      <Flex gap="1" align="center" mt="1">
        {fav.year && (
          <Text size="1" color="gray">
            {fav.year}
          </Text>
        )}
        {fav.rating != null && fav.rating > 0 && (
          <Text size="1" color="amber">
            {"\u2605"}
            {fav.rating.toFixed(1)}
          </Text>
        )}
      </Flex>
    </div>
  );
}

export function Favourites() {
  const { favourites, isLoading, removeFavourite } = useFavourites();
  const [tab, setTab] = useState("all");

  const filtered =
    tab === "all"
      ? favourites
      : favourites.filter((f) => f.content_type === tab);

  return (
    <Flex direction="column" gap="4">
      <Text size="5" weight="bold">
        Favourites
      </Text>

      <Tabs.Root value={tab} onValueChange={setTab}>
        <Tabs.List>
          <Tabs.Trigger value="all">All</Tabs.Trigger>
          <Tabs.Trigger value="movie">Movies</Tabs.Trigger>
          <Tabs.Trigger value="tv">TV Shows</Tabs.Trigger>
          <Tabs.Trigger value="music">Music</Tabs.Trigger>
        </Tabs.List>
      </Tabs.Root>

      {isLoading ? (
        <div
          className="browse-grid"
          style={{ display: "grid", gap: 12 }}
        >
          {Array.from({ length: 8 }).map((_, i) => (
            <div key={i}>
              <Skeleton
                width="100%"
                style={{ aspectRatio: "2/3", borderRadius: 6 }}
              />
              <Skeleton height="12px" width="80%" style={{ marginTop: 6 }} />
            </div>
          ))}
        </div>
      ) : filtered.length === 0 ? (
        <Flex justify="center" py="9">
          <Text size="2" color="gray">
            No favourites yet
          </Text>
        </Flex>
      ) : (
        <div
          className="browse-grid"
          style={{ display: "grid", gap: 12 }}
        >
          {filtered.map((fav) => (
            <FavTile key={fav.id} fav={fav} onRemove={removeFavourite} />
          ))}
        </div>
      )}
    </Flex>
  );
}
