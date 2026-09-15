use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use sterngate_core::{
    decode_from_armor, encode_to_armor, ModAction, ModCategory, ModMetadata, ModRiskLevel,
    ModTargetFilter, SterngateMod,
};
use sterngate_protocol::{ModRunner, TargetFingerprintPolicy};

use super::common::hex_to_bytes;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/mods/inspect", post(mods_inspect))
        .route("/api/v1/mods/apply", post(mods_apply))
        .route("/api/v1/mods/create", post(mods_create))
        .route("/api/v1/mods/library", get(mods_library))
}

#[derive(Deserialize)]
struct ModInspectPayload {
    content: String,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    battery_voltage: Option<f64>,
}

async fn mods_inspect(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ModInspectPayload>,
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

    let modpack = match decode_from_armor(&payload.content) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to parse mod package: {}", e),
                })),
            )
                .into_response();
        }
    };

    let mut iface = state.interface.lock().await;
    match ModRunner::inspect_compatibility(
        &mut **iface,
        &modpack,
        payload.vin.as_deref(),
        // Pass the reading through unchanged: an absent voltage must be
        // reported as unverified, not substituted with a passing default.
        payload.battery_voltage,
    )
    .await
    {
        Ok(report) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "mod": modpack,
                "validation": report,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Inspection failed: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModApplyPayload {
    content: String,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    battery_voltage: Option<f64>,
}

async fn mods_apply(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ModApplyPayload>,
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

    let Some(vin) = payload
        .vin
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "Refusing to apply: no target VIN supplied. The chassis fingerprint is meaningless without the connected vehicle's VIN.",
            })),
        )
            .into_response();
    };

    let mut modpack = match decode_from_armor(&payload.content) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to parse mod package: {}", e),
                })),
            )
                .into_response();
        }
    };

    // Every mod declares its own min_battery_voltage. Defaulting here made
    // that interlock unfailable on a vehicle nothing had measured.
    let Some(voltage) = payload.battery_voltage else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "Refusing to apply: no measured battery voltage supplied. \
                          Send 'battery_voltage' from a real hardware reading.",
            })),
        )
            .into_response();
    };

    let mut iface = state.interface.lock().await;
    match ModRunner::apply_mod(
        &mut **iface,
        &mut modpack,
        vin,
        voltage,
        TargetFingerprintPolicy::Enforce,
    )
    .await
    {
        Ok(exec_report) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "report": exec_report,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to apply mod: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ModCreatePayload {
    name: String,
    author: String,
    description: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    risk_level: Option<String>,
    #[serde(default)]
    chassis: Vec<String>,
    #[serde(default = "default_ecu_name")]
    ecu_name: String,
    #[serde(default)]
    tx_id: Option<u32>,
    #[serde(default)]
    rx_id: Option<u32>,
    #[serde(default)]
    compatible_hw_ids: Vec<String>,
    #[serde(default)]
    min_voltage: Option<f64>,
    #[serde(default)]
    did: u16,
    data_hex: String,
    #[serde(default)]
    bitmask_hex: Option<String>,
    #[serde(default)]
    action_description: Option<String>,
}

fn default_ecu_name() -> String {
    "EDC16".into()
}

async fn mods_create(Json(payload): Json<ModCreatePayload>) -> impl IntoResponse {
    let clean_hex = payload.data_hex.trim().replace([' ', '\n', '\r'], "");
    let data_bytes = match hex_to_bytes(&clean_hex) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Invalid data hex string: {}", e),
                })),
            )
                .into_response();
        }
    };

    let bitmask = if let Some(ref m_hex) = payload.bitmask_hex {
        let clean = m_hex.trim().replace([' ', '\n', '\r'], "");
        hex_to_bytes(&clean).ok()
    } else {
        None
    };

    let mod_id = format!(
        "{}-{}",
        payload
            .name
            .to_lowercase()
            .replace([' ', '_', '/'], "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>(),
        &uuid::Uuid::new_v4().to_string()[..8]
    );

    let category = match payload.category.as_deref().unwrap_or("performance") {
        "transmission" => ModCategory::Transmission,
        "comfort" => ModCategory::Comfort,
        "lighting" => ModCategory::Lighting,
        "brakes" => ModCategory::Brakes,
        "emissions" => ModCategory::Emissions,
        "retrofit" => ModCategory::Retrofit,
        _ => ModCategory::Performance,
    };

    let risk_level = match payload.risk_level.as_deref().unwrap_or("moderate") {
        "low" => ModRiskLevel::Low,
        "high" => ModRiskLevel::High,
        _ => ModRiskLevel::Moderate,
    };

    let metadata = ModMetadata {
        mod_id,
        name: payload.name,
        version: "1.0.0".into(),
        author: payload.author,
        description: payload.description,
        category,
        risk_level,
        instructions: Some("Ignition ON, Engine OFF".into()),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let target = ModTargetFilter {
        chassis: if payload.chassis.is_empty() {
            vec!["W211".into()]
        } else {
            payload.chassis
        },
        ecu_name: payload.ecu_name,
        tx_id: payload.tx_id.unwrap_or(0x7E0),
        rx_id: payload.rx_id.unwrap_or(0x7E8),
        compatible_hw_ids: payload.compatible_hw_ids,
        compatible_sw_ids: vec![],
        min_battery_voltage: payload.min_voltage.unwrap_or(12.0),
        requires_engine_off: true,
    };

    let actions = vec![ModAction::WriteDid {
        did: payload.did,
        data: data_bytes,
        bitmask,
        expected_original_data: None,
        description: payload
            .action_description
            .unwrap_or_else(|| format!("Configure DID 0x{:04X}", payload.did)),
    }];

    match SterngateMod::create(metadata, target, actions, vec![]) {
        Ok(modpack) => {
            let armored = encode_to_armor(&modpack).unwrap_or_default();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "mod": modpack,
                    "armored_text": armored,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed creating mod package: {}", e),
            })),
        )
            .into_response(),
    }
}

async fn mods_library() -> impl IntoResponse {
    let mods_dir = std::path::Path::new("mods");
    let mut list = Vec::new();

    if let Ok(entries) = std::fs::read_dir(mods_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("sgmod") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(modpack) = decode_from_armor(&content) {
                        list.push(modpack);
                    }
                }
            }
        }
    }

    (StatusCode::OK, Json(list)).into_response()
}
