use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use sterngate_core::{Language, VehicleGarage, VehicleProfile};

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/profile", get(get_profile))
        .route("/api/v1/profiles", get(get_all_profiles))
        .route("/api/v1/profile/select", post(select_profile_endpoint))
        .route("/api/v1/ecu/stats", get(get_ecu_stats))
        .route("/api/v1/ecu/search", get(search_ecu_catalog))
        .route("/api/v1/ecu/inspect/{ecu}", get(inspect_ecu_definition))
        .route("/api/v1/cbf/stats", get(get_ecu_stats))
        .route("/api/v1/cbf/search", get(search_ecu_catalog))
        .route("/api/v1/cbf/inspect/{ecu}", get(inspect_ecu_definition))
        .route("/api/v1/locales", get(get_available_locales))
        .route("/api/v1/vehicles", get(list_garage_vehicles))
        .route("/api/v1/vehicles/{vin}", get(get_garage_vehicle))
        .route(
            "/api/v1/vehicles/{vin}/history",
            get(get_vehicle_git_history),
        )
}

#[derive(Deserialize, Default)]
struct ProfileQuery {
    lang: Option<String>,
}

async fn get_profile(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ProfileQuery>,
) -> Json<VehicleProfile> {
    let lang: Language = query
        .lang
        .as_deref()
        .unwrap_or("en")
        .parse()
        .unwrap_or_default();
    let mut prof = state.profile.read().await.clone();
    prof.localize(lang);
    Json(prof)
}

#[derive(Deserialize)]
struct SelectProfilePayload {
    name: String,
}

async fn select_profile_endpoint(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SelectProfilePayload>,
) -> impl IntoResponse {
    let candidates = ["profiles", "../../profiles", "../profiles"];
    let mut found_path = None;

    for dir in candidates {
        let base = std::path::Path::new(dir);
        if !base.exists() {
            continue;
        }

        let direct = base.join(&payload.name);
        if direct.exists() {
            found_path = Some(direct);
            break;
        }

        let with_json = base.join(format!("{}.json", payload.name));
        if with_json.exists() {
            found_path = Some(with_json);
            break;
        }

        let profiles = VehicleProfile::discover(base);
        for prof in profiles {
            if prof.profile_name.eq_ignore_ascii_case(&payload.name)
                || prof
                    .profile_name
                    .to_lowercase()
                    .contains(&payload.name.to_lowercase())
            {
                *state.profile.write().await = prof.clone();
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "success": true,
                        "profile_name": prof.profile_name,
                        "message": format!("Active vehicle profile switched to {}", prof.profile_name),
                    })),
                )
                    .into_response();
            }
        }
    }

    if let Some(path) = found_path {
        match VehicleProfile::load_from_file(&path) {
            Ok(prof) => {
                let name = prof.profile_name.clone();
                *state.profile.write().await = prof;
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "success": true,
                        "profile_name": name,
                        "message": format!("Active vehicle profile switched to {}", name),
                    })),
                )
                    .into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to parse profile {}: {}", path.display(), e),
                })),
            )
                .into_response(),
        }
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Profile '{}' not found", payload.name),
            })),
        )
            .into_response()
    }
}

async fn get_all_profiles() -> impl IntoResponse {
    let candidates = ["profiles", "../../profiles", "../profiles"];
    for dir in candidates {
        let p = std::path::Path::new(dir);
        if p.exists() {
            let profiles = VehicleProfile::discover(p);
            let summaries: Vec<serde_json::Value> = profiles
                .into_iter()
                .map(|prof| {
                    serde_json::json!({
                        "profile_name": prof.profile_name,
                        "oem": prof.oem,
                        "chassis": prof.chassis,
                        "gateway_type": prof.gateway_type,
                        "default_bitrate": prof.default_bitrate,
                        "modules_count": prof.modules.len(),
                        "parameters_count": prof.parameters.len(),
                    })
                })
                .collect();
            return Json(summaries).into_response();
        }
    }
    Json(Vec::<serde_json::Value>::new()).into_response()
}

#[derive(Deserialize)]
struct EcuSearchQuery {
    q: Option<String>,
    limit: Option<usize>,
}

async fn get_ecu_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(catalog) = state.catalog.as_ref() {
        (
            StatusCode::OK,
            Json(serde_json::to_value(catalog.stats()).unwrap()),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "ECU catalog not loaded"
            })),
        )
            .into_response()
    }
}

async fn search_ecu_catalog(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EcuSearchQuery>,
) -> impl IntoResponse {
    if let Some(catalog) = state.catalog.as_ref() {
        let q = query.q.unwrap_or_default();
        let limit = query.limit.unwrap_or(25);
        let results = catalog.search(&q, limit);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "query": q,
                "count": results.len(),
                "results": results,
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "ECU catalog not loaded"
            })),
        )
            .into_response()
    }
}

async fn inspect_ecu_definition(
    State(state): State<Arc<AppState>>,
    Path(ecu): Path<String>,
) -> impl IntoResponse {
    if let Some(catalog) = state.catalog.as_ref() {
        if let Some(entry) = catalog.get_ecu(&ecu) {
            (StatusCode::OK, Json(serde_json::to_value(entry).unwrap())).into_response()
        } else {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("ECU '{}' not found in ECU catalog", ecu)
                })),
            )
                .into_response()
        }
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "ECU catalog not loaded"
            })),
        )
            .into_response()
    }
}

async fn get_available_locales() -> impl IntoResponse {
    Json(vec!["en", "de", "sv"])
}

async fn list_garage_vehicles() -> impl IntoResponse {
    let garage = VehicleGarage::new(VehicleGarage::default_path());
    match garage.list_vehicles() {
        Ok(vehicles) => (StatusCode::OK, Json(vehicles)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to list vehicles: {}", e) })),
        )
            .into_response(),
    }
}

async fn get_garage_vehicle(Path(vin): Path<String>) -> impl IntoResponse {
    let garage = VehicleGarage::new(VehicleGarage::default_path());
    match garage.load_vehicle(&vin) {
        Ok(Some(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Vehicle '{}' not found in garage", vin) })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to load vehicle: {}", e) })),
        )
            .into_response(),
    }
}

async fn get_vehicle_git_history(Path(vin): Path<String>) -> impl IntoResponse {
    let garage = VehicleGarage::new(VehicleGarage::default_path());
    match garage.get_history(&vin) {
        Ok(history) => (StatusCode::OK, Json(history)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to load vehicle history: {}", e) })),
        )
            .into_response(),
    }
}
