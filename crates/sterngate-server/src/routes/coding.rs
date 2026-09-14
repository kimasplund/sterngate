use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use sterngate_core::{EcoStartStopMode, VariantCodingCatalog, VehicleGarage};
use sterngate_protocol::{ServiceRoutineManager, UdsClient, VinAdaptationManager};

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/coding", post(write_coding))
        .route("/api/v1/coding/dids", get(list_variant_coding_dids))
        .route("/api/v1/coding/revin", post(revin_adaptation_endpoint))
        .route("/api/v1/workflow/adblue-reset", post(workflow_adblue_reset))
        .route(
            "/api/v1/workflow/eco-start-stop",
            post(workflow_eco_start_stop),
        )
        .route("/api/v1/workflow/egr-optimize", post(workflow_egr_optimize))
        .route("/api/v1/workflow/vmax", post(workflow_vmax))
        .route(
            "/api/v1/workflow/seatbelt-chime",
            post(workflow_seatbelt_chime),
        )
        .route("/api/v1/workflow/tank-liters", post(workflow_tank_liters))
        .route(
            "/api/v1/workflow/cornering-lights",
            post(workflow_cornering_lights),
        )
        .route("/api/v1/workflows", get(get_workflows_list))
}

#[derive(Serialize)]
struct GenericResponse {
    success: bool,
    message: String,
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
struct AdBlueResetPayload {
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    ecu_tx: Option<u32>,
    #[serde(default)]
    ecu_rx: Option<u32>,
}

async fn workflow_adblue_reset(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AdBlueResetPayload>,
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

    let ecu_tx = payload.ecu_tx.unwrap_or(0x7E0);
    let ecu_rx = payload.ecu_rx.unwrap_or(0x7E8);

    let mut iface = state.interface.lock().await;
    match ServiceRoutineManager::reset_adblue_countdown(&mut **iface, ecu_tx, ecu_rx).await {
        Ok(status) => {
            if status.success {
                let garage = VehicleGarage::new(VehicleGarage::default_path());
                let vin = payload.vin.as_deref().unwrap_or("WDB2112061A000001");
                let note = "AdBlue / SCR emergency 800km countdown and lockout reset executed";
                let _ = garage.save_coding(vin, "SCR_DIAG", "0x0218_RESET_OK", None, note);
            }
            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("AdBlue reset procedure failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct EcoStartStopPayload {
    mode: String,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    ecu_tx: Option<u32>,
    #[serde(default)]
    ecu_rx: Option<u32>,
}

async fn workflow_eco_start_stop(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EcoStartStopPayload>,
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

    let mode = match EcoStartStopMode::parse_str(&payload.mode) {
        Some(m) => m,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!(
                        "Invalid ECO mode '{}'. Valid options: 'always_on', 'remember', 'disabled'",
                        payload.mode
                    ),
                })),
            )
                .into_response();
        }
    };

    let ecu_tx = payload.ecu_tx.unwrap_or(0x7E0);
    let ecu_rx = payload.ecu_rx.unwrap_or(0x7E8);

    let mut iface = state.interface.lock().await;
    match ServiceRoutineManager::configure_eco_start_stop(&mut **iface, ecu_tx, ecu_rx, mode).await
    {
        Ok(status) => {
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let vin = payload.vin.as_deref().unwrap_or("WDB2112061A000001");
            let note = format!("Updated ECO Start-Stop configuration: {}", mode.as_str());
            let _ = garage.save_coding(
                vin,
                &status.module,
                &format!("{:02X}", status.did),
                None,
                &note,
            );

            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("ECO Start-Stop configuration failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct EgrOptimizePayload {
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    ecu_tx: Option<u32>,
    #[serde(default)]
    ecu_rx: Option<u32>,
}

async fn workflow_egr_optimize(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EgrOptimizePayload>,
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

    let ecu_tx = payload.ecu_tx.unwrap_or(0x7E0);
    let ecu_rx = payload.ecu_rx.unwrap_or(0x7E8);

    let mut iface = state.interface.lock().await;
    match ServiceRoutineManager::optimize_egr_adaptation(&mut **iface, ecu_tx, ecu_rx).await {
        Ok(status) => {
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let vin = payload.vin.as_deref().unwrap_or("WDB2112061A000001");
            let note = "EGR adaptation optimized (+40 mg soot reduction offset applied)";
            let _ = garage.save_coding(vin, &status.module, "EGR_AIRMASS_+40MG", None, note);

            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("EGR optimization failed: {}", e),
            })),
        )
            .into_response(),
    }
}

async fn get_workflows_list() -> impl IntoResponse {
    let workflows = serde_json::json!([
        {
            "id": "adblue_countdown_reset",
            "name": "AdBlue / SCR 800km Emergency Lockout Reset",
            "category": "Workshop Maintenance",
            "risk_level": "high",
            "min_voltage": 12.5,
            "requires_engine_off": true,
            "description": "Unlocks engine ECU with Daimler cryptographic Seed-Key (Level 01/0B), wipes permanent SCR start-lockout counter, resets NOx sensor adaptations, and relearns ultrasonic tank level."
        },
        {
            "id": "eco_start_stop_memory",
            "name": "ECO Start-Stop Last State Memory",
            "category": "Vehicle Customization",
            "risk_level": "moderate",
            "min_voltage": 12.0,
            "requires_engine_off": true,
            "description": "Programs engine controller or Front SAM (DID 0x0320) to remember driver's last button selection across ignition cycles instead of defaulting to enabled."
        },
        {
            "id": "egr_soot_optimization",
            "name": "EGR Adaptation Soot Reduction",
            "category": "Powertrain Optimization",
            "risk_level": "moderate",
            "min_voltage": 12.0,
            "requires_engine_off": true,
            "description": "Applies factory-tolerated +40 mg/stroke positive air mass adaptation bias and relearns lower mechanical stops to minimize intake manifold carbon fouling."
        },
        {
            "id": "sbc_brake_pad_service",
            "name": "SBC Hydraulic 0-Bar Pad Service Mode",
            "category": "Workshop Safety",
            "risk_level": "high",
            "min_voltage": 12.5,
            "requires_engine_off": true,
            "description": "Depressurizes ~160 bar accumulator into reservoir, retracts pistons, and suppresses all wake-up triggers to safely replace brake pads without amputation hazard."
        },
        {
            "id": "vmax_speed_limiter",
            "name": "Vehicle Maximum Road Speed Limiter (VMax)",
            "category": "Vehicle Customization",
            "risk_level": "low",
            "min_voltage": 12.0,
            "requires_engine_off": true,
            "description": "Reads and configures road speed governor threshold (DID 0x0110) in engine management system (e.g. 210, 250 km/h or custom limit)."
        },
        {
            "id": "seatbelt_acoustic_chime",
            "name": "Instrument Cluster Seatbelt Acoustic Warning Chime",
            "category": "Vehicle Customization",
            "risk_level": "low",
            "min_voltage": 12.0,
            "requires_engine_off": false,
            "description": "Mutes repetitive audible buzzer in Instrument Cluster (KI DID 0x0201) while preserving all dashboard visual safety lamps and restraint system readiness."
        },
        {
            "id": "tank_liters_display",
            "name": "Remaining Fuel in Liters (Restliteranzeige)",
            "category": "Vehicle Customization",
            "risk_level": "low",
            "min_voltage": 12.0,
            "requires_engine_off": false,
            "description": "Enables exact digital remaining fuel volume in liters display on the central multifunction instrument cluster trip computer screen (DID 0x0205)."
        },
        {
            "id": "cornering_fog_lights",
            "name": "Front SAM Intelligent Cornering Fog Lights (Abbiegelicht)",
            "category": "Lighting & Safety",
            "risk_level": "low",
            "min_voltage": 12.0,
            "requires_engine_off": false,
            "description": "Programs Front SAM (DID 0x0310) to automatically illuminate corresponding fog light when turning indicator is active or steering angle exceeds threshold below 40 km/h."
        }
    ]);

    (StatusCode::OK, Json(workflows))
}

#[derive(Deserialize)]
struct VmaxPayload {
    speed_limit_kmh: u16,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    ecu_tx: Option<u32>,
    #[serde(default)]
    ecu_rx: Option<u32>,
}

async fn workflow_vmax(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<VmaxPayload>,
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

    let ecu_tx = payload.ecu_tx.unwrap_or(0x7E0);
    let ecu_rx = payload.ecu_rx.unwrap_or(0x7E8);

    let mut iface = state.interface.lock().await;
    match ServiceRoutineManager::configure_speed_limiter(
        &mut **iface,
        ecu_tx,
        ecu_rx,
        payload.speed_limit_kmh,
    )
    .await
    {
        Ok(status) => {
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let vin = payload.vin.as_deref().unwrap_or("WDB2112061A000001");
            let note = format!(
                "VMax speed limiter configured to {} km/h",
                payload.speed_limit_kmh
            );
            let _ = garage.save_coding(
                vin,
                &status.module,
                &format!("VMAX_{}KMH", payload.speed_limit_kmh),
                None,
                &note,
            );

            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("VMax speed limiter configuration failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct SeatbeltChimePayload {
    acoustic_enabled: bool,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    ic_tx: Option<u32>,
    #[serde(default)]
    ic_rx: Option<u32>,
}

async fn workflow_seatbelt_chime(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SeatbeltChimePayload>,
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

    let ic_tx = payload.ic_tx.unwrap_or(0x7E4);
    let ic_rx = payload.ic_rx.unwrap_or(0x7EC);

    let mut iface = state.interface.lock().await;
    match ServiceRoutineManager::configure_seatbelt_chime(
        &mut **iface,
        ic_tx,
        ic_rx,
        payload.acoustic_enabled,
    )
    .await
    {
        Ok(status) => {
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let vin = payload.vin.as_deref().unwrap_or("WDB2112061A000001");
            let note = format!(
                "Instrument cluster seatbelt acoustic warning chime {}",
                if payload.acoustic_enabled {
                    "enabled"
                } else {
                    "muted"
                }
            );
            let _ = garage.save_coding(
                vin,
                &status.module,
                if payload.acoustic_enabled {
                    "CHIME_ON"
                } else {
                    "CHIME_MUTED"
                },
                None,
                &note,
            );

            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Seatbelt chime configuration failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct TankLitersPayload {
    enabled: bool,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    ic_tx: Option<u32>,
    #[serde(default)]
    ic_rx: Option<u32>,
}

async fn workflow_tank_liters(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<TankLitersPayload>,
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

    let ic_tx = payload.ic_tx.unwrap_or(0x7E4);
    let ic_rx = payload.ic_rx.unwrap_or(0x7EC);

    let mut iface = state.interface.lock().await;
    match ServiceRoutineManager::configure_tank_liters_display(
        &mut **iface,
        ic_tx,
        ic_rx,
        payload.enabled,
    )
    .await
    {
        Ok(status) => {
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let vin = payload.vin.as_deref().unwrap_or("WDB2112061A000001");
            let note = format!(
                "Instrument cluster exact tank liters display (Restliteranzeige) {}",
                if payload.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            let _ = garage.save_coding(
                vin,
                &status.module,
                if payload.enabled {
                    "RESTLITER_ON"
                } else {
                    "RESTLITER_OFF"
                },
                None,
                &note,
            );

            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Tank liters display configuration failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct CorneringLightsPayload {
    enabled: bool,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    sam_tx: Option<u32>,
    #[serde(default)]
    sam_rx: Option<u32>,
}

async fn workflow_cornering_lights(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CorneringLightsPayload>,
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

    let sam_tx = payload.sam_tx.unwrap_or(0x7E2);
    let sam_rx = payload.sam_rx.unwrap_or(0x7EA);

    let mut iface = state.interface.lock().await;
    match ServiceRoutineManager::configure_cornering_lights(
        &mut **iface,
        sam_tx,
        sam_rx,
        payload.enabled,
    )
    .await
    {
        Ok(status) => {
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let vin = payload.vin.as_deref().unwrap_or("WDB2112061A000001");
            let note = format!(
                "Front SAM intelligent cornering fog lights {}",
                if payload.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            let _ = garage.save_coding(
                vin,
                &status.module,
                if payload.enabled {
                    "CORNERING_FOG_ON"
                } else {
                    "CORNERING_FOG_OFF"
                },
                None,
                &note,
            );

            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Cornering lights configuration failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct CodingListParams {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    ecu: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn list_variant_coding_dids(Query(params): Query<CodingListParams>) -> impl IntoResponse {
    match VariantCodingCatalog::load_default() {
        Ok(cat) => {
            let limit = params.limit.unwrap_or(50);
            let query = params.q.as_deref().unwrap_or("");
            let results = cat.search(query, params.ecu.as_deref(), limit);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "total": results.len(),
                    "coding_dids": results,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to load variant coding catalog: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct RevinPayload {
    target_ecu: String,
    new_vin: String,
    #[serde(default)]
    tx_id: Option<u32>,
    #[serde(default)]
    rx_id: Option<u32>,
    #[serde(default)]
    security_level: Option<u8>,
}

async fn revin_adaptation_endpoint(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RevinPayload>,
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

    let tx_id = payload.tx_id.unwrap_or(0x7E0);
    let rx_id = payload.rx_id.unwrap_or(0x7E8);

    let mut iface_guard = state.interface.lock().await;
    match VinAdaptationManager::adapt_donor_ecu_vin(
        &mut **iface_guard,
        tx_id,
        rx_id,
        &payload.target_ecu,
        &payload.new_vin,
        payload.security_level,
    )
    .await
    {
        Ok(adapt) => {
            if adapt.success {
                let note = format!(
                    "Donor ECU {} Re-VIN adaptation: programmed to {}",
                    payload.target_ecu, payload.new_vin
                );
                let garage = VehicleGarage::new(VehicleGarage::default_path());
                let _ = garage.save_coding(
                    &payload.new_vin,
                    &payload.target_ecu,
                    &payload.new_vin,
                    None,
                    &note,
                );
            }
            (StatusCode::OK, Json(serde_json::to_value(adapt).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Re-VIN adaptation failed: {}", e),
            })),
        )
            .into_response(),
    }
}
