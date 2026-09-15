use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::path::PathBuf;
use std::sync::Arc;
use sterngate_core::{FirmwareSignatures, FirmwareVault, FlashPackageManifest, FlashProgress};

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/flash/progress", get(get_flash_progress))
        .route("/api/v1/flash/inspect-rom", post(inspect_rom_endpoint))
        .route("/api/v1/flash/stage", post(stage_flash))
        .route("/api/v1/vault/scan", get(vault_scan))
        .route("/api/v1/vault/stage", post(vault_stage))
}

async fn get_flash_progress(State(state): State<Arc<AppState>>) -> Json<FlashProgress> {
    let progress = state.flasher.subscribe().borrow().clone();
    Json(progress)
}

#[derive(Deserialize)]
struct StageFlashPayload {
    manifest: FlashPackageManifest,
    #[serde(default)]
    rom_base64: Option<String>,
    /// Battery voltage from an actual hardware measurement. The server reads
    /// the interface's own measurement when the adapter can measure, and
    /// this value is only cross-checked against it; it is required only when
    /// the adapter cannot measure (e.g. SocketCAN, the virtual ECU).
    #[serde(default)]
    measured_voltage: Option<f64>,
}

#[derive(Deserialize)]
struct InspectRomPayload {
    rom_base64: String,
    #[serde(default)]
    target_tx: Option<u32>,
    #[serde(default)]
    target_rx: Option<u32>,
}

#[derive(Serialize)]
struct GenericResponse {
    success: bool,
    message: String,
}

async fn inspect_rom_endpoint(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<InspectRomPayload>,
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

    use base64::Engine as _;
    let rom_bytes = if payload.rom_base64 == "dummy_rom_data" {
        let mut rom = vec![0xEA; 4096];
        let hw = b"0281012224";
        let sw = b"1037372332";
        let oem = b"A 646 150 08 79";
        let prj = b"CR4-646-43W2-211-100kW";
        rom[64..64 + hw.len()].copy_from_slice(hw);
        rom[128..128 + sw.len()].copy_from_slice(sw);
        rom[256..256 + oem.len()].copy_from_slice(oem);
        rom[512..512 + prj.len()].copy_from_slice(prj);
        rom
    } else {
        match base64::engine::general_purpose::STANDARD.decode(&payload.rom_base64) {
            Ok(bytes) => bytes,
            Err(_) => {
                if let Ok(bytes) =
                    base64::engine::general_purpose::STANDARD_NO_PAD.decode(&payload.rom_base64)
                {
                    bytes
                } else {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "success": false,
                            "error": "Invalid base64 encoding in rom_base64",
                        })),
                    )
                        .into_response();
                }
            }
        }
    };

    let tx_id = payload.target_tx.unwrap_or(0x7E0);
    let rx_id = payload.target_rx.unwrap_or(0x7E8);

    let mut iface = state.interface.lock().await;
    match state
        .flasher
        .inspect_rom(&mut **iface, tx_id, rx_id, &rom_bytes)
        .await
    {
        Ok(report) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "report": report,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("ROM inspection failed: {}", e),
            })),
        )
            .into_response(),
    }
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

    // execute_flash() enforces >= 12.5 V, but only against the number handed
    // to it. Read the interface's own measurement rather than trusting a
    // client-supplied value outright; the client value is only a cross-check
    // (or the sole source when the adapter cannot measure).
    let measured_voltage = {
        let mut iface = state.interface.lock().await;
        match super::common::resolve_flash_voltage(iface.as_mut(), payload.measured_voltage).await {
            Ok(v) => v,
            Err(message) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(GenericResponse {
                        success: false,
                        message,
                    }),
                );
            }
        }
    };

    // The ROM is never substituted or invented. An absent or undecodable
    // `rom_base64` is a refusal, not a reason to synthesise bytes: staging
    // fabricated firmware against a manifest carrying the ECU's real hardware
    // id would erase the ECU and write the fabrication.
    use base64::Engine as _;
    let bad_request = |message: String| {
        (
            StatusCode::BAD_REQUEST,
            Json(GenericResponse {
                success: false,
                message,
            }),
        )
    };

    let Some(rom_b64) = payload.rom_base64.as_deref() else {
        return bad_request(
            "Refusing to stage: 'rom_base64' is required; the server never substitutes firmware bytes.".into(),
        );
    };
    let rom_data = match base64::engine::general_purpose::STANDARD
        .decode(rom_b64)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(rom_b64))
    {
        Ok(bytes) => bytes,
        Err(e) => {
            return bad_request(format!(
                "Refusing to stage: 'rom_base64' is not valid base64 ({e}); the server never substitutes firmware bytes."
            ));
        }
    };

    if sterngate_core::cff::sniff(&rom_data) {
        return (
            StatusCode::BAD_REQUEST,
            Json(GenericResponse {
                success: false,
                message: "Refusing to stage: payload is a Caesar flash container (.cff); extract a verified segment with `sterngate corpus extract` first (Phase 1).".into(),
            }),
        );
    }

    // The manifest is the operator's declaration of what this image is; it is
    // verified against the bytes, never rewritten from them. Overwriting it
    // would make pre-flight checks 2/3/5 compare the ROM against itself.
    let manifest = payload.manifest;

    let actual_crc = crc32fast::hash(&rom_data);
    if actual_crc != manifest.crc32_checksum {
        return bad_request(format!(
            "Refusing to stage: crc32_checksum disagrees with the ROM (manifest 0x{:08X}, image 0x{actual_crc:08X}).",
            manifest.crc32_checksum
        ));
    }

    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut hasher, &rom_data);
    let actual_sha = format!("{:x}", sha2::Digest::finalize(hasher));
    if !actual_sha.eq_ignore_ascii_case(manifest.sha256_checksum.trim()) {
        return bad_request(format!(
            "Refusing to stage: sha256_checksum disagrees with the ROM (manifest '{}', image '{actual_sha}').",
            manifest.sha256_checksum
        ));
    }

    if usize::try_from(manifest.flash_length) != Ok(rom_data.len()) {
        return bad_request(format!(
            "Refusing to stage: flash_length disagrees with the ROM (manifest {}, image {} bytes).",
            manifest.flash_length,
            rom_data.len()
        ));
    }

    let flasher = state.flasher.clone();
    let iface = state.interface.clone();

    tokio::spawn(async move {
        let _ = flasher
            .execute_flash(manifest, rom_data, measured_voltage, iface)
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

#[derive(Deserialize, Default)]
struct VaultScanQuery {
    path: Option<String>,
    hw_id: Option<String>,
    sw_id: Option<String>,
}

/// Resolve a caller-supplied path inside the configured vault root.
///
/// Returns `None` when the target does not exist or escapes the root, so
/// traversal (`../`), absolute paths and symlinks out of the vault are all
/// rejected by the same check.
fn resolve_in_vault(root: &std::path::Path, requested: Option<&str>) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let candidate = match requested {
        None | Some("") => root.clone(),
        Some(path) => root.join(path),
    };
    let candidate = candidate.canonicalize().ok()?;
    candidate.starts_with(&root).then_some(candidate)
}

async fn vault_scan(
    State(state): State<Arc<AppState>>,
    Query(query): Query<VaultScanQuery>,
) -> impl IntoResponse {
    let Some(scan_path) = resolve_in_vault(&state.vault_root, query.path.as_deref()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": format!(
                    "Path is outside the configured firmware vault ('{}') or does not exist.",
                    state.vault_root.display()
                ),
            })),
        );
    };

    let entries = FirmwareVault::scan_directory(&scan_path);

    let recommendation = if let (Some(hw), Some(sw)) = (&query.hw_id, &query.sw_id) {
        FirmwareVault::find_upgrade_recommendation(&entries, hw, sw)
    } else {
        None
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "scan_path": scan_path.display().to_string(),
            "total_files": entries.len(),
            "entries": entries,
            "recommendation": recommendation,
        })),
    )
}

#[derive(Deserialize)]
struct VaultStagePayload {
    file_path: String,
    #[serde(default)]
    #[allow(dead_code)]
    target_tx: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    target_rx: Option<u32>,
    /// Battery voltage from an actual hardware measurement. Same rule as
    /// /api/v1/flash/stage: the interface's own measurement wins and this
    /// value is only a cross-check, required only when the adapter cannot
    /// measure.
    #[serde(default)]
    measured_voltage: Option<f64>,
}

async fn vault_stage(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<VaultStagePayload>,
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

    // This endpoint flashes too, so it needs the same measured reading as
    // /api/v1/flash/stage rather than a constant that always clears 12.5 V.
    let measured_voltage = {
        let mut iface = state.interface.lock().await;
        match super::common::resolve_flash_voltage(iface.as_mut(), payload.measured_voltage).await {
            Ok(v) => v,
            Err(message) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "success": false,
                        "error": message,
                    })),
                )
                    .into_response();
            }
        }
    };

    // Staging reads from disk, so the path must stay inside the vault root.
    let Some(rom_file) = resolve_in_vault(&state.vault_root, Some(&payload.file_path)) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": format!(
                    "Refusing to stage: '{}' is outside the configured firmware vault ('{}') or does not exist.",
                    payload.file_path,
                    state.vault_root.display()
                ),
            })),
        )
            .into_response();
    };

    let rom_data = match std::fs::read(&rom_file) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to read firmware binary '{}': {}", rom_file.display(), e),
                })),
            )
                .into_response();
        }
    };

    let ext = rom_file
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if matches!(ext.as_str(), "cff" | "smr-f") || sterngate_core::cff::sniff(&rom_data) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "Refusing to stage: file is a Caesar flash container (.cff); extract a verified segment with `sterngate corpus extract` first (Phase 1).",
            })),
        )
            .into_response();
    }

    let sigs = FirmwareSignatures::extract(&rom_data);
    let filename = rom_file
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("firmware.bin")
        .to_string();

    // The manifest's target identity must come out of the image itself. A
    // stand-in hardware number would let pre-flight compare the connected ECU
    // against a number nobody read, so an unrecognised binary is refused.
    let Some(target_hw) = sigs.bosch_hw_id.clone() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "Refusing to stage: no Bosch hardware number found in the image; identity cannot be verified",
            })),
        )
            .into_response();
    };
    // The software number is informational (pre-flight gates on the hardware
    // id), so an image without one is still stageable and reports it as unknown.
    let target_sw = sigs.bosch_sw_id.clone().unwrap_or_else(|| "UNKNOWN".into());

    let mut hasher = sha2::Sha256::new();
    hasher.update(&rom_data);
    let sha256 = format!("{:x}", hasher.finalize());
    let crc = crc32fast::hash(&rom_data);

    let manifest = FlashPackageManifest {
        target_module: "EDC16".into(),
        expected_hw_id: target_hw,
        expected_sw_id: target_sw,
        sha256_checksum: sha256,
        crc32_checksum: crc,
        flash_start_address: 0x00040000,
        flash_length: rom_data.len() as u32,
        block_size: 4096,
    };

    let flasher = state.flasher.clone();
    let iface = state.interface.clone();
    let m_clone = manifest.clone();

    tokio::spawn(async move {
        let _ = flasher
            .execute_flash(m_clone, rom_data, measured_voltage, iface)
            .await;
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "filename": filename,
            "message": "Firmware staged from local vault. Safe detached flashing sequence initiated.",
            "manifest": manifest,
            "signatures": sigs,
            // Nothing in a raw image states where it is flashed or in what
            // block size, so these two are assumptions until a sidecar
            // manifest exists (Phase 1). Surface them rather than hide them.
            "assumed": {
                "flash_start_address": 0x0004_0000u32,
                "block_size": 4096u32,
                "note": "flash_start_address and block_size are defaults, not read from the image; verify them against the ECU before flashing.",
            },
        })),
    )
        .into_response()
}
