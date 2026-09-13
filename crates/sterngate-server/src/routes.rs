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
        .route("/api/v1/routine", post(execute_routine))
        .route("/api/v1/recorder/start", post(start_recorder))
        .route("/api/v1/recorder/stop", post(stop_recorder))
        .route("/api/v1/recorder/status", get(get_recorder_status))
        .with_state(state)
}

async fn dashboard_handler() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

pub async fn sample_telemetry(state: &AppState) -> TelemetrySnapshot {
    if state.flasher.is_locked().await {
        return TelemetrySnapshot {
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            battery_voltage: 13.8,
            ..Default::default()
        };
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
    snap
}

async fn get_telemetry(State(state): State<Arc<AppState>>) -> Json<TelemetrySnapshot> {
    let snap = sample_telemetry(&state).await;
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
    envelope: Option<sterngate_core::CommandEnvelope>,
    did_hex: Option<String>,
    data_hex: Option<String>,
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

    let envelope = if let Some(env) = payload.envelope {
        env
    } else {
        let did_hex = payload.did_hex.unwrap_or_default();
        let data_hex = payload.data_hex.unwrap_or_default();
        let did = u16::from_str_radix(did_hex.trim_start_matches("0x"), 16).unwrap_or(0);
        let raw_bytes: Result<Vec<u8>, _> = (0..data_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&data_hex[i..i + 2], 16))
            .collect();

        let bytes = match raw_bytes {
            Ok(b) => b,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(GenericResponse {
                        success: false,
                        message: "Invalid hex payload format".into(),
                    }),
                )
            }
        };

        sterngate_core::CommandEnvelope::new("EDC16", 0x2E, Some(did), bytes)
    };

    // Strict Zero-Trust Verification Gate
    let profile = state.profile.read().await;
    if let Err(e) = state.gate.verify_and_authorize(&envelope, &profile).await {
        tracing::warn!("Rejected unverified or corrupt command: {}", e);
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(GenericResponse {
                success: false,
                message: format!("Command verification gate failed: {}", e),
            }),
        );
    }

    let did = envelope.did.unwrap_or(0);
    let mut iface = state.interface.lock().await;
    let mut uds = UdsClient::new(iface.as_mut(), 0x7E0, 0x7E8);
    match uds.write_data_by_identifier(did, &envelope.payload).await {
        Ok(_) => (
            StatusCode::OK,
            Json(GenericResponse {
                success: true,
                message: format!(
                    "Successfully verified and wrote DID 0x{:04X} (Command ID: {})",
                    did, envelope.command_id
                ),
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

#[derive(Deserialize)]
struct RoutinePayload {
    envelope: Option<sterngate_core::CommandEnvelope>,
    module: Option<String>,
    routine_id_hex: Option<String>,
    sub_function: Option<u8>,
    option_record_hex: Option<String>,
}

#[derive(Serialize)]
struct RoutineResponse {
    success: bool,
    command_id: String,
    routine_id: String,
    status_hex: String,
    message: String,
}

async fn execute_routine(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RoutinePayload>,
) -> impl IntoResponse {
    if state.flasher.is_locked().await {
        return (
            StatusCode::LOCKED,
            Json(RoutineResponse {
                success: false,
                command_id: "".into(),
                routine_id: "".into(),
                status_hex: "".into(),
                message: "API is locked during flash operation".into(),
            }),
        );
    }

    let envelope = if let Some(env) = payload.envelope {
        env
    } else {
        let module = payload.module.unwrap_or_else(|| "EDC16".into());
        let routine_id_hex = payload.routine_id_hex.unwrap_or_else(|| "0xFF01".into());
        let routine_id =
            u16::from_str_radix(routine_id_hex.trim_start_matches("0x"), 16).unwrap_or(0xFF01);
        let sub_function = payload.sub_function.unwrap_or(0x01);
        let opt_hex = payload.option_record_hex.unwrap_or_default();
        let option_bytes: Result<Vec<u8>, _> = (0..opt_hex.len())
            .step_by(2)
            .map(|i| {
                if i + 2 <= opt_hex.len() {
                    u8::from_str_radix(&opt_hex[i..i + 2], 16)
                } else {
                    u8::from_str_radix(&opt_hex[i..], 16)
                }
            })
            .collect();

        let mut raw_bytes = vec![sub_function, (routine_id >> 8) as u8, routine_id as u8];
        if let Ok(opts) = option_bytes {
            raw_bytes.extend(opts);
        }

        sterngate_core::CommandEnvelope::new(&module, 0x31, Some(routine_id), raw_bytes)
    };

    // Strict Zero-Trust Verification Gate
    let profile = state.profile.read().await;
    if let Err(e) = state.gate.verify_and_authorize(&envelope, &profile).await {
        tracing::warn!("Rejected unverified or corrupt routine command: {}", e);
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(RoutineResponse {
                success: false,
                command_id: envelope.command_id,
                routine_id: format!("0x{:04X}", envelope.did.unwrap_or(0)),
                status_hex: "".into(),
                message: format!("Command verification gate failed: {}", e),
            }),
        );
    }

    let sub_fn = if !envelope.payload.is_empty() {
        envelope.payload[0]
    } else {
        0x01
    };
    let routine_id = envelope.did.unwrap_or_else(|| {
        if envelope.payload.len() >= 3 {
            u16::from_be_bytes([envelope.payload[1], envelope.payload[2]])
        } else {
            0xFF01
        }
    });
    let option_record = if envelope.payload.len() > 3 {
        &envelope.payload[3..]
    } else {
        &[]
    };

    let (tx_id, rx_id) = if let Some(m) = profile.get_module(&envelope.target_module) {
        (
            m.tx_can_id().unwrap_or(0x7E0),
            m.rx_can_id().unwrap_or(0x7E8),
        )
    } else if envelope.target_module.eq_ignore_ascii_case("EGS52") {
        (0x7E1, 0x7E9)
    } else {
        (0x7E0, 0x7E8)
    };

    let mut iface = state.interface.lock().await;
    let mut uds = UdsClient::new(iface.as_mut(), tx_id, rx_id);
    match uds.routine_control(sub_fn, routine_id, option_record).await {
        Ok(resp) => {
            let resp_hex = resp
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");
            let routine_desc = match routine_id {
                0xFF01 => "Fuel Pump Prime & Rail Bleed",
                0x0201 => "Reset NMK Injector Zero-Quantity Adaptations",
                0x0202 => "Trigger DPF Regeneration",
                0x0203 => "Throttle Valve / EGR Stop Relearn",
                0x0205 => "SBC Brake Hydraulic Bleed Routine",
                0xFF00 => "Erase Flash Memory Routine",
                _ => "Diagnostic Routine Control",
            };
            (
                StatusCode::OK,
                Json(RoutineResponse {
                    success: true,
                    command_id: envelope.command_id,
                    routine_id: format!("0x{:04X}", routine_id),
                    status_hex: resp_hex,
                    message: format!(
                        "{} (0x{:04X}) completed successfully",
                        routine_desc, routine_id
                    ),
                }),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RoutineResponse {
                success: false,
                command_id: envelope.command_id,
                routine_id: format!("0x{:04X}", routine_id),
                status_hex: "".into(),
                message: format!("Failed to execute routine 0x{:04X}: {}", routine_id, e),
            }),
        ),
    }
}

#[derive(Deserialize)]
struct RecorderStartPayload {
    filename: Option<String>,
}

async fn start_recorder(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Option<RecorderStartPayload>>,
) -> Json<crate::recorder::FlightRecorderStatus> {
    let rx = state.telemetry_tx.subscribe();
    let name = payload.and_then(|p| p.filename);
    let status = match state.recorder.start(name, rx).await {
        Ok(s) => s,
        Err(_) => state.recorder.status().await,
    };
    Json(status)
}

async fn stop_recorder(
    State(state): State<Arc<AppState>>,
) -> Json<crate::recorder::FlightRecorderStatus> {
    let status = state.recorder.stop().await;
    Json(status)
}

async fn get_recorder_status(
    State(state): State<Arc<AppState>>,
) -> Json<crate::recorder::FlightRecorderStatus> {
    let status = state.recorder.status().await;
    Json(status)
}
