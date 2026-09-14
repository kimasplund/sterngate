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
    CascadeTelemetryInput, CascadeWatchdog, DriveBenchmark, DriveSummary, SuspensionLeakDetector,
    SuspensionSample,
};
use sterngate_protocol::VehicleScanner;

use crate::routes::telemetry::sample_telemetry;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
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
