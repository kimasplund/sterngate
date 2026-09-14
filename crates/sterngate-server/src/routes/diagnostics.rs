use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use sterngate_core::{Dtc, EcuCatalog, Language, VehicleGarage};
use sterngate_protocol::{BusDiscoverer, UdsClient, VehicleScanner};

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/dtc", get(get_dtcs))
        .route("/api/v1/dtc/clear", post(clear_dtcs))
        .route("/api/v1/vehicle/scan", post(scan_vehicle_quick_test))
        .route("/api/v1/diag/discover", post(diag_discover))
        .route("/api/v1/diag/report.html", get(get_diag_report_html))
}

#[derive(Deserialize, Default)]
struct DtcQuery {
    lang: Option<String>,
}

async fn get_dtcs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DtcQuery>,
) -> Json<Vec<Dtc>> {
    let lang: Language = query
        .lang
        .as_deref()
        .unwrap_or("en")
        .parse()
        .unwrap_or_default();

    let mut iface = state.interface.lock().await;
    let mut uds = UdsClient::new(iface.as_mut(), 0x7E0, 0x7E8);
    let mut dtcs = uds
        .read_dtc_information(0x02, "EDC16")
        .await
        .unwrap_or_default();

    for d in &mut dtcs {
        d.localize(lang);
    }
    Json(dtcs)
}

async fn clear_dtcs(State(state): State<Arc<AppState>>) -> StatusCode {
    let mut iface = state.interface.lock().await;
    let mut uds = UdsClient::new(iface.as_mut(), 0x7E0, 0x7E8);
    let _ = uds.clear_diagnostic_information(0xFFFFFF).await;
    StatusCode::OK
}

fn default_lang_en() -> String {
    "en".into()
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct ScanVehicleRequest {
    #[serde(default = "default_lang_en")]
    lang: String,
    #[serde(default = "default_true")]
    save_to_garage: bool,
}

async fn scan_vehicle_quick_test(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Option<ScanVehicleRequest>>,
) -> impl IntoResponse {
    let req = payload.unwrap_or(ScanVehicleRequest {
        lang: "en".into(),
        save_to_garage: true,
    });
    let language: Language = req.lang.parse().unwrap_or_default();

    let mut iface = state.interface.lock().await;
    match VehicleScanner::scan(&mut **iface, language).await {
        Ok(report) => {
            if req.save_to_garage {
                let garage = VehicleGarage::new(VehicleGarage::default_path());
                let rec = report.to_vehicle_record();
                let _ = garage.save_vehicle(&rec, Some("web_scan: quick test completed"));
            }
            (StatusCode::OK, Json(serde_json::to_value(report).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Vehicle scan failed: {}", e) })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct BusDiscoverPayload {
    #[serde(default)]
    start_id: Option<u32>,
    #[serde(default)]
    end_id: Option<u32>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

async fn diag_discover(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Option<BusDiscoverPayload>>,
) -> impl IntoResponse {
    if state.flasher.is_locked().await {
        return (
            StatusCode::LOCKED,
            Json(serde_json::json!({
                "success": false,
                "error": "System is locked in a flashing routine",
            })),
        )
            .into_response();
    }

    let req = payload.unwrap_or(BusDiscoverPayload {
        start_id: None,
        end_id: None,
        timeout_ms: None,
    });
    let start = req.start_id.unwrap_or(0x7E0);
    let end = req.end_id.unwrap_or(0x7EF);
    let timeout = req.timeout_ms.unwrap_or(20);

    let catalog_res = EcuCatalog::load_default();
    let catalog_ref = catalog_res.as_ref().ok();

    let mut iface = state.interface.lock().await;
    match BusDiscoverer::discover_ecus(&mut **iface, start..=end, timeout, catalog_ref).await {
        Ok(ecus) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "count": ecus.len(),
                "ecus": ecus,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Bus discovery failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ReportHtmlQuery {
    #[serde(default)]
    lang: Option<String>,
}

async fn get_diag_report_html(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ReportHtmlQuery>,
) -> impl IntoResponse {
    if state.flasher.is_locked().await {
        return (
            StatusCode::LOCKED,
            [("content-type", "text/plain; charset=utf-8")],
            "System is locked in a flashing routine".to_string(),
        )
            .into_response();
    }

    let language: Language = query
        .lang
        .as_deref()
        .unwrap_or("en")
        .parse()
        .unwrap_or_default();
    let mut iface = state.interface.lock().await;
    match VehicleScanner::scan(&mut **iface, language).await {
        Ok(report) => {
            let html = report.to_html(language);
            (
                StatusCode::OK,
                [("content-type", "text/html; charset=utf-8")],
                html,
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "text/plain; charset=utf-8")],
            format!("Diagnostic scan failed: {}", e),
        )
            .into_response(),
    }
}
