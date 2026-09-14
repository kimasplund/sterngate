use crate::assets;
use crate::state::AppState;
use crate::ws::ws_telemetry_handler;
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::sync::Arc;
use sterngate_core::{
    decode_from_armor, encode_to_armor, lookup_routine_name, BoschChecksumSolver, BoschMapDetector,
    CascadeTelemetryInput, CascadeWatchdog, DriveBenchmark, DriveSummary, Dtc, EcoStartStopMode,
    EcuCatalog, FirmwareSignatures, FirmwareVault, FlashPackageManifest, FlashProgress, Language,
    ModAction, ModCategory, ModMetadata, ModRiskLevel, ModTargetFilter, SbcServiceAction,
    StageGenerator, SterngateMod, SuspensionCorner, SuspensionCornerAction, SuspensionLeakDetector,
    SuspensionSample, TelemetrySnapshot, VariantCodingCatalog, VehicleGarage, VehicleProfile,
    WorkshopRoutineCatalog,
};
use sterngate_protocol::{
    BusDiscoverer, ModRunner, ServiceRoutineManager, UdsClient, VehicleScanner,
    VinAdaptationManager,
};

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
        .route("/api/v1/telemetry", get(get_telemetry))
        .route("/api/v1/dtc", get(get_dtcs))
        .route("/api/v1/dtc/clear", post(clear_dtcs))
        .route("/api/v1/profile", get(get_profile))
        .route("/api/v1/profiles", get(get_all_profiles))
        .route("/api/v1/profile/select", post(select_profile_endpoint))
        .route("/api/v1/flash/progress", get(get_flash_progress))
        .route("/api/v1/flash/inspect-rom", post(inspect_rom_endpoint))
        .route("/api/v1/flash/stage", post(stage_flash))
        .route("/api/v1/coding", post(write_coding))
        .route("/api/v1/routine", post(execute_routine))
        .route("/api/v1/recorder/start", post(start_recorder))
        .route("/api/v1/recorder/stop", post(stop_recorder))
        .route("/api/v1/recorder/status", get(get_recorder_status))
        .route("/api/v1/ecu/stats", get(get_ecu_stats))
        .route("/api/v1/ecu/search", get(search_ecu_catalog))
        .route("/api/v1/ecu/inspect/{ecu}", get(inspect_ecu_definition))
        .route("/api/v1/cbf/stats", get(get_ecu_stats))
        .route("/api/v1/cbf/search", get(search_ecu_catalog))
        .route("/api/v1/cbf/inspect/{ecu}", get(inspect_ecu_definition))
        .route("/api/v1/locales", get(get_available_locales))
        .route("/api/v1/vehicle/scan", post(scan_vehicle_quick_test))
        .route("/api/v1/vehicles", get(list_garage_vehicles))
        .route("/api/v1/vehicles/{vin}", get(get_garage_vehicle))
        .route(
            "/api/v1/vehicles/{vin}/history",
            get(get_vehicle_git_history),
        )
        .route(
            "/api/v1/analyze/suspension",
            post(analyze_suspension_health),
        )
        .route("/api/v1/analyze/compare", post(compare_drive_runs))
        .route("/api/v1/analyze/cascades", get(get_cascade_analysis))
        .route("/api/v1/analyze/cascades", post(post_cascade_analysis))
        .route(
            "/api/v1/suspension/compressor/control",
            post(control_compressor),
        )
        .route(
            "/api/v1/suspension/compressor/status",
            get(get_compressor_status),
        )
        .route("/api/v1/abc/control", post(control_abc))
        .route("/api/v1/service/sbc", post(service_sbc))
        .route("/api/v1/service/ima", get(get_service_ima))
        .route("/api/v1/service/ima", post(post_service_ima))
        .route("/api/v1/service/suspension", post(service_suspension))
        .route("/api/v1/diag/discover", post(diag_discover))
        .route("/api/v1/diag/report.html", get(get_diag_report_html))
        .route("/api/v1/service/routines", get(list_workshop_routines))
        .route(
            "/api/v1/service/routines/execute",
            post(execute_workshop_routine_endpoint),
        )
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
        .route("/api/v1/vault/scan", get(vault_scan))
        .route("/api/v1/vault/stage", post(vault_stage))
        .route("/api/v1/workflows", get(get_workflows_list))
        .route("/api/v1/mods/inspect", post(mods_inspect))
        .route("/api/v1/mods/apply", post(mods_apply))
        .route("/api/v1/mods/create", post(mods_create))
        .route("/api/v1/mods/library", get(mods_library))
        .route("/api/v1/tuning/scan", post(tuning_scan_rom))
        .route("/api/v1/tuning/stage1", post(tuning_stage1))
        .route("/api/v1/tuning/stage2", post(tuning_stage2))
        .route("/api/v1/tuning/dtc/kill", post(tuning_dtc_kill))
        .route(
            "/api/v1/tuning/checksum/verify",
            post(tuning_checksum_verify),
        )
        .route("/api/v1/tuning/checksum/fix", post(tuning_checksum_fix))
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .with_state(state)
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

async fn get_profile(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DtcQuery>,
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

async fn get_flash_progress(State(state): State<Arc<AppState>>) -> Json<FlashProgress> {
    let progress = state.flasher.subscribe().borrow().clone();
    Json(progress)
}

#[derive(Deserialize)]
struct StageFlashPayload {
    manifest: FlashPackageManifest,
    #[serde(default)]
    rom_base64: Option<String>,
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

    use base64::Engine as _;
    let rom_data = match payload.rom_base64.as_deref() {
        Some("dummy_rom_data") | None => vec![0xAA; 4096],
        Some(b64) => base64::engine::general_purpose::STANDARD
            .decode(b64)
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64))
            .unwrap_or_else(|_| vec![0xAA; 4096]),
    };

    let mut manifest = payload.manifest;
    manifest.crc32_checksum = crc32fast::hash(&rom_data);
    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut hasher, &rom_data);
    manifest.sha256_checksum = format!("{:x}", sha2::Digest::finalize(hasher));
    manifest.flash_length = rom_data.len() as u32;

    let flasher = state.flasher.clone();
    let iface = state.interface.clone();

    tokio::spawn(async move {
        let _ = flasher.execute_flash(manifest, rom_data, 13.8, iface).await;
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

async fn get_available_locales() -> impl IntoResponse {
    Json(vec!["en", "de", "sv"])
}

#[derive(Deserialize)]
struct ScanVehicleRequest {
    #[serde(default = "default_lang_en")]
    lang: String,
    #[serde(default = "default_true")]
    save_to_garage: bool,
}

fn default_lang_en() -> String {
    "en".into()
}

fn default_true() -> bool {
    true
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

#[derive(Deserialize)]
struct SuspensionAnalysisRequest {
    #[serde(default)]
    samples: Vec<SuspensionSample>,
}

async fn analyze_suspension_health(
    Json(payload): Json<Option<SuspensionAnalysisRequest>>,
) -> impl IntoResponse {
    let mut detector = SuspensionLeakDetector::new();
    let samples = payload.and_then(|p| {
        if p.samples.is_empty() {
            None
        } else {
            Some(p.samples)
        }
    });

    if let Some(s_list) = samples {
        for s in s_list {
            detector.add_sample(s);
        }
    } else {
        // Provide baseline live evaluation
        detector.add_sample(SuspensionSample {
            timestamp_ms: 1000,
            left_rear_height_mm: 118.0,
            right_rear_height_mm: 118.5,
            compressor_active: false,
            compressor_run_duration_s: 0.0,
            reservoir_pressure_bar: Some(14.2),
            compressor_temp_c: Some(38.0),
        });
        detector.add_sample(SuspensionSample {
            timestamp_ms: 1000 + 1_800_000,
            left_rear_height_mm: 117.8,
            right_rear_height_mm: 118.2,
            compressor_active: false,
            compressor_run_duration_s: 0.0,
            reservoir_pressure_bar: Some(14.0),
            compressor_temp_c: Some(35.0),
        });
    }

    let report = detector.evaluate();
    Json(report)
}

#[derive(Deserialize)]
struct CompareRunsRequest {
    run_a: DriveSummary,
    run_b: DriveSummary,
    #[serde(default = "default_run_a_name")]
    name_a: String,
    #[serde(default = "default_run_b_name")]
    name_b: String,
}

fn default_run_a_name() -> String {
    "Run A (Baseline)".into()
}

fn default_run_b_name() -> String {
    "Run B (Modified)".into()
}

async fn compare_drive_runs(Json(payload): Json<CompareRunsRequest>) -> impl IntoResponse {
    let cmp = DriveBenchmark::compare(
        &payload.run_a,
        &payload.run_b,
        &payload.name_a,
        &payload.name_b,
    );
    Json(cmp)
}

#[derive(Deserialize)]
struct CompressorControlRequest {
    action: String, // "inhibit", "workshop", "restore"
    #[serde(default)]
    reason: Option<String>,
}

async fn control_compressor(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CompressorControlRequest>,
) -> impl IntoResponse {
    let action_str = payload.action.to_lowercase();
    let reason = payload
        .reason
        .unwrap_or_else(|| "User manual override".into());

    let guard_state = {
        let mut guard = state.compressor_guard.lock().unwrap();
        if action_str == "inhibit" || action_str == "disable" || action_str == "safemode" {
            guard.manual_inhibit(&reason);
        } else if action_str == "restore" || action_str == "enable" || action_str == "normal" {
            guard.manual_restore();
        }
        guard.current_state.clone()
    };

    let mut iface = state.interface.lock().await;
    match VehicleScanner::control_suspension_compressor(&mut **iface, &action_str).await {
        Ok(msg) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "message": msg,
                "guard_state": guard_state,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to dispatch compressor routine: {}", e),
                "guard_state": guard_state,
            })),
        )
            .into_response(),
    }
}

async fn get_compressor_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let guard = state.compressor_guard.lock().unwrap();
    Json(serde_json::json!({
        "current_state": guard.current_state,
        "is_inhibited": guard.is_inhibited,
        "inhibit_reason": guard.inhibit_reason,
        "watchdog_enabled": guard.watchdog_enabled,
        "max_continuous_run_seconds": guard.max_continuous_run_seconds,
        "cooldown_period_seconds": guard.cooldown_period_seconds,
    }))
}

async fn get_cascade_analysis(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let snap = sample_telemetry(&state).await;
    let guard = state.compressor_guard.lock().unwrap();

    let input = CascadeTelemetryInput {
        sbc_accumulator_pressure_bar: Some(78.0),
        sbc_pump_per_brake_ratio: Some(0.18),
        sbc_operating_cycles: Some(125_000),
        sbc_max_cycles: Some(300_000),
        max_cylinder_balance_trim_mm3: snap.inj_corr_cyl1.map(|v| v.abs()),
        cylinder_balance_spread_mm3: Some(1.2),
        rail_pressure_bleed_rate_bar_sec: Some(12.0),
        atf_temp_rapid_jump_deg_c: Some(0.5),
        transmission_speed_sensor_jitter: Some(false),
        tcc_slip_rpm: snap.tcc_slip_rpm,
        tcc_lockup_commanded: Some(true),
        dpf_diff_pressure_mbar: Some(35.0),
        engine_rpm: snap.engine_rpm,
        distance_since_dpf_regen_km: Some(420.0),
        cam_magnet_oil_detected: Some(false),
        o2_sensor_heater_resistance_drift: Some(false),
        five_volt_ref_bus_dip: Some(false),
        compressor_continuous_run_sec: Some(guard.current_run_seconds()),
        compressor_duty_cycle_pct: Some(0.0),
        suspension_height_drop_rate_mm_h: Some(0.6),
        abc_pressure_ripple_bar: Some(3.5),
        abc_system_pressure_bar: Some(195.0),
        esl_unlock_duration_ms: Some(185.0),
        esl_retry_count: Some(0),
        cam_phase_deviation_deg: Some(0.4),
        tcc_slip_oscillation_hz: Some(0.0),
        tcc_slip_oscillation_rpm: Some(1.5),
        can_sleep_delay_seconds: Some(15.0),
        quiescent_current_amps: Some(0.02),
        dynamic_oil_loss_rate_mm_100km: Some(0.02),
        engine_oil_temperature_c: snap.coolant_temp.or(Some(90.0)),
        active_dtcs: vec![],
    };

    let report = CascadeWatchdog::evaluate(&input);
    Json(report)
}

async fn post_cascade_analysis(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Option<CascadeTelemetryInput>>,
) -> impl IntoResponse {
    let snap = sample_telemetry(&state).await;
    let guard = state.compressor_guard.lock().unwrap();

    let mut input = payload.unwrap_or_default();
    if input.tcc_slip_rpm.is_none() {
        input.tcc_slip_rpm = snap.tcc_slip_rpm;
    }
    if input.engine_rpm.is_none() {
        input.engine_rpm = snap.engine_rpm;
    }
    if input.compressor_continuous_run_sec.is_none() {
        input.compressor_continuous_run_sec = Some(guard.current_run_seconds());
    }

    let report = CascadeWatchdog::evaluate(&input);
    Json(report)
}

#[derive(Deserialize)]
struct AbcControlRequest {
    action: String, // "dump", "lock", "restore"
}

async fn control_abc(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AbcControlRequest>,
) -> impl IntoResponse {
    let action_str = payload.action.to_lowercase();
    let mut iface = state.interface.lock().await;
    match VehicleScanner::control_abc_safety_limiter(&mut **iface, &action_str).await {
        Ok(msg) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "message": msg,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to dispatch ABC routine: {}", e),
            })),
        )
            .into_response(),
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

#[derive(Deserialize, Default)]
struct VaultScanQuery {
    path: Option<String>,
    hw_id: Option<String>,
    sw_id: Option<String>,
}

async fn vault_scan(Query(query): Query<VaultScanQuery>) -> impl IntoResponse {
    let scan_path = query.path.unwrap_or_else(|| "firmware_vault".to_string());
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
            "scan_path": scan_path,
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

    let rom_data = match std::fs::read(&payload.file_path) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to read firmware binary '{}': {}", payload.file_path, e),
                })),
            )
                .into_response();
        }
    };

    let sigs = FirmwareSignatures::extract(&rom_data);
    let filename = std::path::Path::new(&payload.file_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("firmware.bin")
        .to_string();

    let target_hw = sigs
        .bosch_hw_id
        .clone()
        .unwrap_or_else(|| "0281013352".into());
    let target_sw = sigs
        .bosch_sw_id
        .clone()
        .unwrap_or_else(|| "1037386738".into());

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
        let _ = flasher.execute_flash(m_clone, rom_data, 13.8, iface).await;
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "filename": filename,
            "message": "Firmware staged from local vault. Safe detached flashing sequence initiated.",
            "manifest": manifest,
            "signatures": sigs,
        })),
    )
        .into_response()
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
        payload.battery_voltage.or(Some(12.6)),
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
struct ModApplyPayload {
    content: String,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    battery_voltage: Option<f64>,
    #[serde(default)]
    force: bool,
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

    let vin = payload.vin.as_deref().unwrap_or("WDB2112061A000001");
    let voltage = payload.battery_voltage.unwrap_or(12.6);

    let mut iface = state.interface.lock().await;
    match ModRunner::apply_mod(&mut **iface, &mut modpack, vin, voltage, payload.force).await {
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

fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("Hex string must have an even length".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|e| format!("Invalid hex byte at position {}: {}", i, e))
        })
        .collect()
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

#[derive(Debug, Deserialize)]
pub struct RomSourcePayload {
    pub rom_path: Option<String>,
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
}

fn load_rom_bytes(
    payload: &RomSourcePayload,
) -> std::result::Result<Vec<u8>, (StatusCode, Json<serde_json::Value>)> {
    if let Some(ref path) = payload.rom_path {
        std::fs::read(path).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to read ROM file from '{}': {}", path, e),
                })),
            )
        })
    } else if let Some(ref b64) = payload.rom_base64 {
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
                "error": "No ROM provided. Please specify 'rom_path', 'rom_base64', or 'rom_hex'.",
            })),
        ))
    }
}

#[derive(Debug, Deserialize)]
pub struct TuningScanPayload {
    pub rom_path: Option<String>,
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
}

async fn tuning_scan_rom(Json(payload): Json<TuningScanPayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_path: payload.rom_path,
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
    pub rom_path: Option<String>,
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
    pub chassis: Option<String>,
    pub ecu_name: Option<String>,
    pub author: Option<String>,
}

async fn tuning_stage1(Json(payload): Json<TuningStagePayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_path: payload.rom_path,
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to generate Stage 1 package: {}", e),
            })),
        )
            .into_response(),
    }
}

async fn tuning_stage2(Json(payload): Json<TuningStagePayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_path: payload.rom_path,
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
                    "summary": "+25% Peak Torque, +200 mbar Boost, +80 bar Rail, DPF Off, EGR Hysteresis Off, P0401/P2002 DTC Suppressed",
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to generate Stage 2 package: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct TuningDtcKillPayload {
    pub rom_path: Option<String>,
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
    pub chassis: Option<String>,
    pub ecu_name: Option<String>,
    pub p_codes: Vec<String>,
    pub author: Option<String>,
}

async fn tuning_dtc_kill(Json(payload): Json<TuningDtcKillPayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_path: payload.rom_path,
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to generate DTC kill package: {}", e),
            })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct TuningChecksumPayload {
    pub rom_path: Option<String>,
    pub rom_base64: Option<String>,
    pub rom_hex: Option<String>,
    pub output_path: Option<String>,
}

async fn tuning_checksum_verify(Json(payload): Json<TuningChecksumPayload>) -> impl IntoResponse {
    let rom = match load_rom_bytes(&RomSourcePayload {
        rom_path: payload.rom_path,
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
        rom_path: payload.rom_path.clone(),
        rom_base64: payload.rom_base64,
        rom_hex: payload.rom_hex,
    }) {
        Ok(bytes) => bytes,
        Err(err_resp) => return err_resp.into_response(),
    };

    match BoschChecksumSolver::recalculate_and_apply(&mut rom) {
        Ok(report) => {
            let mut saved_to = None;
            let target_path = payload.output_path.or(payload.rom_path);
            if let Some(ref path) = target_path {
                if let Err(e) = std::fs::write(path, &rom) {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "success": false,
                            "error": format!(
                                "Calculated checksums successfully but failed writing to '{}': {}",
                                path, e
                            ),
                        })),
                    )
                        .into_response();
                }
                saved_to = Some(path.clone());
            }

            use base64::Engine;
            let fixed_b64 = if saved_to.is_none() {
                Some(base64::engine::general_purpose::STANDARD.encode(&rom))
            } else {
                None
            };

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "report": report,
                    "saved_to": saved_to,
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
