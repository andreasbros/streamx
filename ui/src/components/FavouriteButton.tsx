import { useFavourites } from "../hooks/useFavourites";
import type { SearchResultGroup } from "../api/types";

interface FavouriteButtonProps {
  group: SearchResultGroup;
  size?: number;
  style?: React.CSSProperties;
}

export function FavouriteButton({ group, size = 28, style }: FavouriteButtonProps) {
  const { isFavourite, addFavourite, removeFavouriteByTitle } = useFavourites();
  const active = isFavourite(group.title, group.year);

  const handleClick = async (e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    if (active) {
      await removeFavouriteByTitle(group.title, group.year);
    } else {
      await addFavourite({
        content_type: "movie",
        title: group.title,
        year: group.year ?? null,
        rating: group.rating ?? null,
        poster_url: group.poster_medium ?? group.poster ?? null,
        info_hash: null,
        metadata_json: JSON.stringify({
          genres: group.genres,
          summary: group.summary,
          imdb_code: group.imdb_code,
          trailer_code: group.trailer_code,
          poster_small: group.poster_small,
          poster_medium: group.poster_medium,
          poster_large: group.poster_large,
          backdrop: group.backdrop,
          variants: group.variants,
        }),
      });
    }
  };

  return (
    <div
      onClick={handleClick}
      style={{
        width: size,
        height: size,
        borderRadius: "50%",
        background: active ? "rgba(250, 204, 21, 0.9)" : "rgba(0, 0, 0, 0.5)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        cursor: "pointer",
        transition: "background 0.15s",
        ...style,
      }}
      title={active ? "Remove from favourites" : "Add to favourites"}
    >
      <svg
        width={size * 0.5}
        height={size * 0.5}
        viewBox="0 0 24 24"
        fill={active ? "#fff" : "none"}
        stroke={active ? "#fff" : "#fff"}
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
      </svg>
    </div>
  );
}
