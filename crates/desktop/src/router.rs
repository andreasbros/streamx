//! Page stack router. See also `AppState::navigate` / `back`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Login,
    Search,
    CategoryBrowse,
    Movie,
    Player,
    Loading,
    History,
    Downloads,
    Favourites,
    Settings,
    Logs,
    Admin,
    MusicSearch,
    MusicPlayer,
    TvSearch,
    TvShow,
    MusicVideoSearch,
    SurroundSound,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Page::Login => "Sign in",
            Page::Search => "Movies",
            Page::CategoryBrowse => "Browse",
            Page::Movie => "Movie",
            Page::Player => "Player",
            Page::Loading => "Loading",
            Page::History => "History",
            Page::Downloads => "Downloads",
            Page::Favourites => "Favourites",
            Page::Settings => "Settings",
            Page::Logs => "Logs",
            Page::Admin => "Admin",
            Page::MusicSearch => "Music",
            Page::MusicPlayer => "Now playing",
            Page::TvSearch => "TV shows",
            Page::TvShow => "Show",
            Page::MusicVideoSearch => "Music videos",
            Page::SurroundSound => "Surround sound",
        }
    }
}
