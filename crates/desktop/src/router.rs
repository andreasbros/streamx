//! Minimal page stack router. See also `AppState::navigate` / `back`.

#[derive(Debug, Clone)]
pub enum Page {
    Login,
    Search,
    Movie, // selected_movie held on AppState
    Loading,
}

impl PartialEq for Page {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Login, Self::Login)
                | (Self::Search, Self::Search)
                | (Self::Movie, Self::Movie)
                | (Self::Loading, Self::Loading)
        )
    }
}
