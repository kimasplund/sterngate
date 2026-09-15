use axum::{http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use serde::Deserialize;
use std::sync::Arc;
use sterngate_core::SterngateError;
use sterngate_core::{
    encode_to_armor, BoschChecksumSolver, BoschMapDetector, FirmwareSignatures, StageGenerator,
};

use super::common::hex_to_bytes;
use crate::state::AppState;

/// Maps a generation failure to its HTTP status: a `PreFlightCheckFailed`
/// refusal (unsupported feature, unlocated map, sub-floor voltage) is a
/// client-facing 422; anything else is an unexpected server error.
fn generation_error(prefix: &str, e: &SterngateError) -> axum::response::Response {
    let status = if matches!(e, SterngateError::PreFlightCheckFailed(_)) {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (
        status,
        Json(serde_json::json!({
            "success": false,
            "error": format!("{prefix}: {e}"),
        })),
    )
        .into_response()
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/tuning/scan", post(tuning_scan_rom))
        .route("/api/v1/tuning/stage1", post(tuning_stage1))
        .route("/api/v1/tuning/stage2", post(tuning_stage2))
        .route("/api/v1/tuning/dtc/kill", post(tuning_dtc_kill))
        .route(
            "/api/v1/tuning/checksum/verify",
            post(tuning_checksum_verify),
        )
        .route("/api/v1/tuning/checksum/fix", post(tuning_checksum_fix))
}

#[derive(Debug, Deserialize)]
pub struct RomSourcePayload {
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
}

fn load_rom_bytes(
    payload: &RomSourcePayload,
) -> std::result::Result<Vec<u8>, (StatusCode, Json<serde_json::Value>)> {
    // Deliberately no filesystem path input: these routes are reachable by any
    // client on the network, so a caller-supplied path would be an arbitrary
    // file read. ROMs must arrive in the request body.
    if let Some(ref b64) = payload.rom_base64 {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "success": false,
                        "error": format!("Invalid base64 ROM dump: {}", e),
                    })),
                )
            })
    } else if let Some(ref hex) = payload.rom_hex {
        hex_to_bytes(hex.trim()).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Invalid hex ROM dump: {}", e),
                })),
            )
        })
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "No ROM provided. Please specify 'rom_base64' or 'rom_hex'.",
            })),
        ))
    }
}

#[derive(Debug, Deserialize)]
pub struct TuningScanPayload {
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
}

async fn tuning_scan_rom(Json(payload): Json<TuningScanPayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_base64: payload.rom_base64,
        rom_hex: payload.rom_hex,
    }) {
        Ok(bytes) => bytes,
        Err(err_resp) => return err_resp.into_response(),
    };

    let sigs = FirmwareSignatures::extract(&rom);
    let checksum = BoschChecksumSolver::verify(&rom);
    let maps = BoschMapDetector::scan_rom(&rom);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "rom_size": rom.len(),
            "signatures": sigs,
            "checksum": checksum,
            "map_count": maps.len(),
            "maps": maps,
        })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct TuningStagePayload {
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
    pub chassis: Option<String>,
    pub ecu_name: Option<String>,
    pub author: Option<String>,
}

async fn tuning_stage1(Json(payload): Json<TuningStagePayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_base64: payload.rom_base64,
        rom_hex: payload.rom_hex,
    }) {
        Ok(bytes) => bytes,
        Err(err_resp) => return err_resp.into_response(),
    };

    let chassis = payload.chassis.unwrap_or_else(|| "W211 E280 CDI".into());
    let ecu_name = payload.ecu_name.unwrap_or_else(|| "EDC16CP31".into());
    let author = payload
        .author
        .unwrap_or_else(|| "Sterngate Community Tuner".into());

    match StageGenerator::generate_stage1(&rom, &chassis, &ecu_name, &author) {
        Ok(modpack) => {
            let armored_text = encode_to_armor(&modpack).unwrap_or_default();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "stage": 1,
                    "mod": modpack,
                    "armored_text": armored_text,
                    "summary": "+18% Peak Torque, +120 mbar Boost, +50 bar Rail Pressure",
                })),
            )
                .into_response()
        }
        Err(e) => generation_error("Failed to generate Stage 1 package", &e),
    }
}

async fn tuning_stage2(Json(payload): Json<TuningStagePayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_base64: payload.rom_base64,
        rom_hex: payload.rom_hex,
    }) {
        Ok(bytes) => bytes,
        Err(err_resp) => return err_resp.into_response(),
    };

    let chassis = payload.chassis.unwrap_or_else(|| "W211 E280 CDI".into());
    let ecu_name = payload.ecu_name.unwrap_or_else(|| "EDC16CP31".into());
    let author = payload
        .author
        .unwrap_or_else(|| "Sterngate Community Tuner".into());

    match StageGenerator::generate_stage2(&rom, &chassis, &ecu_name, &author) {
        Ok(modpack) => {
            let armored_text = encode_to_armor(&modpack).unwrap_or_default();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "stage": 2,
                    "mod": modpack,
                    "armored_text": armored_text,
                    "summary": "+25% Peak Torque, +200 mbar Boost, +80 bar Rail, DPF Off, EGR Hysteresis Off",
                })),
            )
                .into_response()
        }
        Err(e) => generation_error("Failed to generate Stage 2 package", &e),
    }
}

#[derive(Debug, Deserialize)]
pub struct TuningDtcKillPayload {
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
    pub chassis: Option<String>,
    pub ecu_name: Option<String>,
    pub p_codes: Vec<String>,
    pub author: Option<String>,
}

async fn tuning_dtc_kill(Json(payload): Json<TuningDtcKillPayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_base64: payload.rom_base64,
        rom_hex: payload.rom_hex,
    }) {
        Ok(bytes) => bytes,
        Err(err_resp) => return err_resp.into_response(),
    };

    let chassis = payload.chassis.unwrap_or_else(|| "W211".into());
    let ecu_name = payload.ecu_name.unwrap_or_else(|| "EDC16".into());
    let author = payload.author.unwrap_or_else(|| "Sterngate Tuner".into());

    match StageGenerator::generate_dtc_kill(&rom, &chassis, &ecu_name, &payload.p_codes, &author) {
        Ok(modpack) => {
            let armored_text = encode_to_armor(&modpack).unwrap_or_default();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "mod": modpack,
                    "armored_text": armored_text,
                    "killed_codes": payload.p_codes,
                })),
            )
                .into_response()
        }
        Err(e) => generation_error("Failed to generate DTC kill package", &e),
    }
}

#[derive(Debug, Deserialize)]
pub struct TuningChecksumPayload {
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
}

async fn tuning_checksum_verify(Json(payload): Json<TuningChecksumPayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_base64: payload.rom_base64,
        rom_hex: payload.rom_hex,
    }) {
        Ok(bytes) => bytes,
        Err(err_resp) => return err_resp.into_response(),
    };

    let report = BoschChecksumSolver::verify(&rom);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "report": report,
        })),
    )
        .into_response()
}

async fn tuning_checksum_fix(Json(payload): Json<TuningChecksumPayload>) -> impl IntoResponse {
    let mut rom = match load_rom_bytes(&RomSourcePayload {
        rom_base64: payload.rom_base64,
        rom_hex: payload.rom_hex,
    }) {
        Ok(bytes) => bytes,
        Err(err_resp) => return err_resp.into_response(),
    };

    match BoschChecksumSolver::recalculate_and_apply(&mut rom) {
        Ok(report) => {
            // The corrected ROM is returned to the caller rather than written
            // to a caller-supplied path, which was an arbitrary file write.
            use base64::Engine;
            let fixed_b64 = base64::engine::general_purpose::STANDARD.encode(&rom);

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "report": report,
                    "fixed_base64": fixed_b64,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed recalculating Bosch checksums: {}", e),
            })),
        )
            .into_response(),
    }
}
