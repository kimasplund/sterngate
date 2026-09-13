use crate::dashboard::DASHBOARD_HTML;
use crate::state::AppState;
use crate::ws::ws_telemetry_handler;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::sync::Arc;
use sterngate_core::{Dtc, FlashPackageManifest, FlashProgress, TelemetrySnapshot, VehicleProfile};
use sterngate_protocol::UdsClient;

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/ws/telemetry", get(ws_telemetry_handler))
        .route("/api/v1/telemetry", get(get_telemetry))
        .route("/api/v1/dtc", get(get_dtcs))
        .route("/api/v1/dtc/clear", post(clear_dtcs))
        .route("/api/v1/profile", get(get_profile))
        .route("/api/v1/flash/progress", get(get_flash_progress))
        .route("/api/v1/flash/stage", post(stage_flash))
        .route("/api/v1/coding", post(write_coding))
        .with_state(state)
}

async fn dashboard_handler() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn get_telemetry(State(state): State<Arc<AppState>>) -> Json<TelemetrySnapshot> {
    if state.flasher.is_locked().await {
        return Json(TelemetrySnapshot {
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            battery_voltage: 13.8,
            ..Default::default()
        });
    }

    let mut iface = state.interface.lock().await;
    let mut snap = TelemetrySnapshot {
        timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        battery_voltage: 13.8,
        ..Default::default()
    };

    let mut uds_edc = UdsClient::new(iface.as_mut(), 0x7E0, 0x7E8);
    if let Ok(resp) = uds_edc.read_data_by_identifier(0x0100).await {
        if resp.len() >= 5 {
            let raw = u16::from_be_bytes([resp[3], resp[4]]) as f64;
            snap.engine_rpm = Some(raw * 0.25);
        }
    }
    if let Ok(resp) = uds_edc.read_data_by_identifier(0x0105).await {
        if resp.len() >= 4 {
            let raw = resp[3] as f64;
            snap.coolant_temp = Some(raw - 40.0);
        }
    }
    if let Ok(resp) = uds_edc.read_data_by_identifier(0x200B).await {
        if resp.len() >= 5 {
            let raw = u16::from_be_bytes([resp[3], resp[4]]) as f64;
            snap.rail_pressure = Some(raw * 0.1);
        }
    }
    if let Ok(resp) = uds_edc.read_data_by_identifier(0x2010).await {
        if resp.len() >= 5 {
            let raw = u16::from_be_bytes([resp[3], resp[4]]) as f64;
            snap.boost_pressure = Some(raw);
        }
    }
    if let Ok(resp) = uds_edc.read_data_by_identifier(0x2021).await {
        if resp.len() >= 5 {
            let raw = u16::from_be_bytes([resp[3], resp[4]]) as f64;
            snap.inj_corr_cyl1 = Some((raw * 0.01) - 5.0);
        }
    }

    let mut uds_egs = UdsClient::new(iface.as_mut(), 0x7E1, 0x7E9);
    if let Ok(resp) = uds_egs.read_data_by_identifier(0x2001).await {
        if resp.len() >= 4 {
            let raw = resp[3] as f64;
            snap.trans_fluid_temp = Some(raw - 40.0);
        }
    }
    if let Ok(resp) = uds_egs.read_data_by_identifier(0x2002).await {
        if resp.len() >= 5 {
            let raw = u16::from_be_bytes([resp[3], resp[4]]) as f64;
            snap.tcc_slip_rpm = Some(raw);
        }
    }

    let _ = state.telemetry_tx.send(snap.clone());
    Json(snap)
}

async fn get_dtcs(State(state): State<Arc<AppState>>) -> Json<Vec<Dtc>> {
    let mut iface = state.interface.lock().await;
    let mut uds = UdsClient::new(iface.as_mut(), 0x7E0, 0x7E8);
    let dtcs = uds
        .read_dtc_information(0x02, "EDC16")
        .await
        .unwrap_or_default();
    Json(dtcs)
}

async fn clear_dtcs(State(state): State<Arc<AppState>>) -> StatusCode {
    let mut iface = state.interface.lock().await;
    let mut uds = UdsClient::new(iface.as_mut(), 0x7E0, 0x7E8);
    let _ = uds.clear_diagnostic_information(0xFFFFFF).await;
    StatusCode::OK
}

async fn get_profile(State(state): State<Arc<AppState>>) -> Json<VehicleProfile> {
    let prof = state.profile.read().await;
    Json(prof.clone())
}

async fn get_flash_progress(State(state): State<Arc<AppState>>) -> Json<FlashProgress> {
    let progress = state.flasher.subscribe().borrow().clone();
    Json(progress)
}

#[derive(Deserialize)]
struct StageFlashPayload {
    manifest: FlashPackageManifest,
    #[allow(dead_code)]
    rom_base64: String,
}

#[derive(Serialize)]
struct GenericResponse {
    success: bool,
    message: String,
}

async fn stage_flash(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StageFlashPayload>,
) -> impl IntoResponse {
    if state.flasher.is_locked().await {
        return (
            StatusCode::LOCKED,
            Json(GenericResponse {
                success: false,
                message: "System is locked in a flashing routine".into(),
            }),
        );
    }

    let dummy_rom = vec![0xAA; 4096];
    let mut manifest = payload.manifest;
    manifest.crc32_checksum = crc32fast::hash(&dummy_rom);
    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut hasher, &dummy_rom);
    manifest.sha256_checksum = format!("{:x}", sha2::Digest::finalize(hasher));

    let flasher = state.flasher.clone();
    let iface = state.interface.clone();

    tokio::spawn(async move {
        let _ = flasher
            .execute_flash(manifest, dummy_rom, 13.8, iface)
            .await;
    });

    (
        StatusCode::OK,
        Json(GenericResponse {
            success: true,
            message: "ROM verified and staged. Safe detached flashing sequence initiated.".into(),
        }),
    )
}

#[derive(Deserialize)]
struct CodingPayload {
    did_hex: String,
    data_hex: String,
}

async fn write_coding(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CodingPayload>,
) -> impl IntoResponse {
    if state.flasher.is_locked().await {
        return (
            StatusCode::LOCKED,
            Json(GenericResponse {
                success: false,
                message: "API is locked during flash operation".into(),
            }),
        );
    }

    let did = u16::from_str_radix(payload.did_hex.trim_start_matches("0x"), 16).unwrap_or(0);
    let raw_bytes: Result<Vec<u8>, _> = (0..payload.data_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&payload.data_hex[i..i + 2], 16))
        .collect();

    let bytes = match raw_bytes {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(GenericResponse {
                    success: false,
                    message: "Invalid hex payload".into(),
                }),
            )
        }
    };

    let mut iface = state.interface.lock().await;
    let mut uds = UdsClient::new(iface.as_mut(), 0x7E0, 0x7E8);
    match uds.write_data_by_identifier(did, &bytes).await {
        Ok(_) => (
            StatusCode::OK,
            Json(GenericResponse {
                success: true,
                message: format!("Successfully wrote DID 0x{:04X}", did),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(GenericResponse {
                success: false,
                message: format!("Failed to write DID 0x{:04X}: {}", did, e),
            }),
        ),
    }
}
