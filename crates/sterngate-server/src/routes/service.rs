use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use sterngate_core::{
    lookup_routine_name, Language, SbcServiceAction, SuspensionCorner, SuspensionCornerAction,
    VehicleGarage, WorkshopRoutineCatalog,
};
use sterngate_protocol::{ServiceRoutineManager, UdsClient};

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/routine", post(execute_routine))
        .route("/api/v1/service/sbc", post(service_sbc))
        .route("/api/v1/service/ima", get(get_service_ima))
        .route("/api/v1/service/ima", post(post_service_ima))
        .route("/api/v1/service/suspension", post(service_suspension))
        .route("/api/v1/service/routines", get(list_workshop_routines))
        .route(
            "/api/v1/service/routines/execute",
            post(execute_workshop_routine_endpoint),
        )
}

#[derive(Deserialize)]
struct RoutinePayload {
    envelope: Option<sterngate_core::CommandEnvelope>,
    module: Option<String>,
    routine_id_hex: Option<String>,
    sub_function: Option<u8>,
    option_record_hex: Option<String>,
    lang: Option<String>,
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

    let lang: Language = payload
        .lang
        .as_deref()
        .unwrap_or("en")
        .parse()
        .unwrap_or_default();

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
            let routine_desc = lookup_routine_name(routine_id, lang);
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
struct SbcServicePayload {
    action: SbcServiceAction,
    #[serde(default)]
    tx_id: Option<u32>,
    #[serde(default)]
    rx_id: Option<u32>,
}

async fn service_sbc(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SbcServicePayload>,
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

    let tx_id = payload.tx_id.unwrap_or(0x7E2);
    let rx_id = payload.rx_id.unwrap_or(0x7EA);

    let mut iface = state.interface.lock().await;
    let res = match payload.action {
        SbcServiceAction::Deactivate => {
            ServiceRoutineManager::deactivate_sbc(&mut **iface, tx_id, rx_id).await
        }
        SbcServiceAction::Reactivate => {
            ServiceRoutineManager::reactivate_sbc(&mut **iface, tx_id, rx_id).await
        }
        SbcServiceAction::Bleed => Err(sterngate_core::SterngateError::ProtocolError(
            "Guided SBC bleeding sequence must be executed via interactive workshop mode".into(),
        )),
    };

    match res {
        Ok(status) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "status": status,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("SBC service routine failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ImaQuery {
    #[serde(default)]
    ecu_tx: Option<u32>,
    #[serde(default)]
    ecu_rx: Option<u32>,
    #[serde(default)]
    cylinder_count: Option<u8>,
    #[serde(default)]
    cylinder: Option<u8>,
}

async fn get_service_ima(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ImaQuery>,
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

    let ecu_tx = query.ecu_tx.unwrap_or(0x7E0);
    let ecu_rx = query.ecu_rx.unwrap_or(0x7E8);

    let mut iface = state.interface.lock().await;

    if let Some(cyl) = query.cylinder {
        match ServiceRoutineManager::read_injector_ima(&mut **iface, ecu_tx, ecu_rx, cyl).await {
            Ok(code) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "injectors": vec![code],
                })),
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to read injector IMA code: {}", e),
                })),
            )
                .into_response(),
        }
    } else {
        let count = query.cylinder_count.unwrap_or(4).clamp(1, 8);
        let mut results = Vec::new();
        for cyl in 1..=count {
            match ServiceRoutineManager::read_injector_ima(&mut **iface, ecu_tx, ecu_rx, cyl).await
            {
                Ok(code) => results.push(code),
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "success": false,
                            "error": format!("Failed to read injector {} IMA: {}", cyl, e),
                        })),
                    )
                        .into_response();
                }
            }
        }
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "injectors": results,
            })),
        )
            .into_response()
    }
}

#[derive(Deserialize)]
struct ImaWritePayload {
    cylinder: u8,
    code: String,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    ecu_tx: Option<u32>,
    #[serde(default)]
    ecu_rx: Option<u32>,
}

async fn post_service_ima(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ImaWritePayload>,
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
    match ServiceRoutineManager::write_injector_ima(
        &mut **iface,
        ecu_tx,
        ecu_rx,
        payload.cylinder,
        &payload.code,
    )
    .await
    {
        Ok(ima) => {
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let vin = payload.vin.as_deref().unwrap_or("WDB2112061A000001");
            let note = format!(
                "Calibrated injector IMA code for cylinder {}: {}",
                payload.cylinder, ima.code
            );
            let _ = garage.save_coding(vin, "EDC16", &ima.code, None, &note);

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "injector": ima,
                    "message": format!(
                        "Successfully programmed cylinder {} IMA code to {}",
                        payload.cylinder, ima.code
                    ),
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to write injector IMA code: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct SuspensionCornerPayload {
    corner: SuspensionCorner,
    action: SuspensionCornerAction,
    #[serde(default)]
    tx_id: Option<u32>,
    #[serde(default)]
    rx_id: Option<u32>,
}

async fn service_suspension(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SuspensionCornerPayload>,
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

    let tx_id = payload.tx_id.unwrap_or(0x7E3);
    let rx_id = payload.rx_id.unwrap_or(0x7EB);

    let mut iface = state.interface.lock().await;
    match ServiceRoutineManager::actuate_suspension_corner(
        &mut **iface,
        tx_id,
        rx_id,
        payload.corner,
        payload.action,
    )
    .await
    {
        Ok(msg) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "corner": payload.corner,
                "action": payload.action,
                "message": msg,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Suspension corner actuation failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct RoutineListParams {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    ecu: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn list_workshop_routines(Query(params): Query<RoutineListParams>) -> impl IntoResponse {
    match WorkshopRoutineCatalog::load_default() {
        Ok(cat) => {
            let limit = params.limit.unwrap_or(50);
            let query = params.q.as_deref().unwrap_or("");
            let results = cat.search(query, params.ecu.as_deref(), limit);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "total": results.len(),
                    "routines": results,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to load workshop routine catalog: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ExecuteRoutinePayload {
    routine_id: String,
    #[serde(default)]
    tx_id: Option<u32>,
    #[serde(default)]
    rx_id: Option<u32>,
    #[serde(default)]
    data_hex: Option<String>,
}

async fn execute_workshop_routine_endpoint(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExecuteRoutinePayload>,
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

    let clean_id = payload.routine_id.trim();
    let stripped = clean_id
        .strip_prefix("0x")
        .or_else(|| clean_id.strip_prefix("0X"))
        .unwrap_or(clean_id);
    let r_id = match u16::from_str_radix(stripped, 16) {
        Ok(val) => val,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Invalid routine ID hex '{}': {}", clean_id, e),
                })),
            )
                .into_response();
        }
    };

    let data_bytes = if let Some(hex_str) = &payload.data_hex {
        hex::decode(hex_str.replace(' ', "")).unwrap_or_default()
    } else {
        vec![]
    };

    let mut iface_guard = state.interface.lock().await;
    match ServiceRoutineManager::execute_generic_routine(
        &mut **iface_guard,
        tx_id,
        rx_id,
        r_id,
        &data_bytes,
    )
    .await
    {
        Ok(res_bytes) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "routine_id": format!("0x{:04X}", r_id),
                "response_hex": hex::encode(&res_bytes),
                "message": format!("Routine 0x{:04X} executed successfully", r_id),
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "routine_id": format!("0x{:04X}", r_id),
                "error": format!("Routine execution failed: {}", e),
            })),
        )
            .into_response(),
    }
}
