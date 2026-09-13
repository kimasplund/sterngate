use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

pub const INDEX_HTML: &str = include_str!("../static/index.html");
pub const STYLE_CSS: &str = include_str!("../static/css/style.css");
pub const ENVELOPE_JS: &str = include_str!("../static/js/envelope.js");
pub const I18N_JS: &str = include_str!("../static/js/i18n.js");
pub const APP_JS: &str = include_str!("../static/js/app.js");
pub const LOCALE_EN: &str = include_str!("../static/locales/en.json");
pub const LOCALE_DE: &str = include_str!("../static/locales/de.json");
pub const LOCALE_SV: &str = include_str!("../static/locales/sv.json");

pub async fn serve_index() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        INDEX_HTML,
    )
        .into_response()
}

pub async fn serve_css() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/css; charset=utf-8"),
        )],
        STYLE_CSS,
    )
        .into_response()
}

pub async fn serve_envelope_js() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/javascript; charset=utf-8"),
        )],
        ENVELOPE_JS,
    )
        .into_response()
}

pub async fn serve_i18n_js() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/javascript; charset=utf-8"),
        )],
        I18N_JS,
    )
        .into_response()
}

pub async fn serve_app_js() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/javascript; charset=utf-8"),
        )],
        APP_JS,
    )
        .into_response()
}

pub async fn serve_locale_en() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )],
        LOCALE_EN,
    )
        .into_response()
}

pub async fn serve_locale_de() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )],
        LOCALE_DE,
    )
        .into_response()
}

pub async fn serve_locale_sv() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )],
        LOCALE_SV,
    )
        .into_response()
}
