use axum::{extract::DefaultBodyLimit, routing::get, Router};
use std::sync::Arc;

use crate::assets;
use crate::state::AppState;
use crate::ws::ws_telemetry_handler;

pub mod analytics;
pub mod coding;
pub mod common;
pub mod community_mods;
pub mod diagnostics;
pub mod flashing;
pub mod service;
pub mod telemetry;
pub mod tuning;
pub mod vehicle;

pub use telemetry::sample_telemetry;

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(assets::serve_index))
        .route("/static/css/style.css", get(assets::serve_css))
        .route("/static/js/envelope.js", get(assets::serve_envelope_js))
        .route("/static/js/i18n.js", get(assets::serve_i18n_js))
        .route("/static/js/app.js", get(assets::serve_app_js))
        .route("/static/locales/en.json", get(assets::serve_locale_en))
        .route("/static/locales/de.json", get(assets::serve_locale_de))
        .route("/static/locales/sv.json", get(assets::serve_locale_sv))
        .route("/ws/telemetry", get(ws_telemetry_handler))
        .merge(telemetry::router())
        .merge(diagnostics::router())
        .merge(vehicle::router())
        .merge(flashing::router())
        .merge(service::router())
        .merge(coding::router())
        .merge(analytics::router())
        .merge(community_mods::router())
        .merge(tuning::router())
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .with_state(state)
}
