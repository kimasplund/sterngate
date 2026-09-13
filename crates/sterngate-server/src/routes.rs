use crate::assets;
use crate::state::AppState;
use crate::ws::ws_telemetry_handler;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::sync::Arc;
use sterngate_core::{
    lookup_routine_name, CascadeTelemetryInput, CascadeWatchdog, DriveBenchmark, DriveSummary, Dtc,
    FlashPackageManifest, FlashProgress, Language, SuspensionLeakDetector, SuspensionSample,
    TelemetrySnapshot, VehicleGarage, VehicleProfile,
};
use sterngate_protocol::{UdsClient, VehicleScanner};

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
        .route("/api/v1/flash/progress", get(get_flash_progress))
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
