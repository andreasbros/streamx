use crate::embedded::Asset;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};

pub async fn static_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if let Some(content) = Asset::get(path) {
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();

        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (
                    header::CACHE_CONTROL,
                    "public, max-age=31536000, immutable".to_string(),
                ),
            ],
            content.data.to_vec(),
        )
            .into_response()
    } else if let Some(index) = Asset::get("index.html") {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string())],
            index.data.to_vec(),
        )
            .into_response()
    } else {
        Html(
            "<html><body>\
             <h1>StreamX</h1>\
             <p>Frontend assets are not available. \
             Build the UI with <code>cd ui &amp;&amp; pnpm build</code> \
             and restart the server.</p>\
             </body></html>"
                .to_string(),
        )
        .into_response()
    }
}
