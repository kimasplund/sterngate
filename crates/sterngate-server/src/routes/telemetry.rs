use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use sterngate_core::TelemetrySnapshot;
use sterngate_protocol::UdsClient;

use crate::recorder::FlightRecorderStatus;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/telemetry", get(get_telemetry))
        .route("/api/v1/recorder/start", post(start_recorder))
        .route("/api/v1/recorder/stop", post(stop_recorder))
        .route("/api/v1/recorder/status", get(get_recorder_status))
}

pub async fn sample_telemetry(state: &AppState) -> TelemetrySnapshot {
    if state.flasher.is_locked().await {
        return TelemetrySnapshot {
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            battery_voltage: None,
            ..Default::default()
        };
    }

    let mut iface = state.interface.lock().await;
    // No voltage source: `VehicleInterface` exposes no voltage read, so the
    // reading stays unknown. (OpenPort measures Pin 16 via an inherent method
    // that is not part of the trait; wiring it through is the follow-up.)
    // Reporting a plausible constant here would silently satisfy the >= 12.5 V
    // flashing interlock on a vehicle nothing has measured.
    let mut snap = TelemetrySnapshot {
        timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        battery_voltage: None,
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

#[derive(Deserialize)]
struct RecorderStartPayload {
    filename: Option<String>,
}

async fn start_recorder(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Option<RecorderStartPayload>>,
) -> Json<FlightRecorderStatus> {
    let rx = state.telemetry_tx.subscribe();
    let name = payload.and_then(|p| p.filename);
    let status = match state.recorder.start(name, rx).await {
        Ok(s) => s,
        Err(_) => state.recorder.status().await,
    };
    Json(status)
}

async fn stop_recorder(State(state): State<Arc<AppState>>) -> Json<FlightRecorderStatus> {
    let status = state.recorder.stop().await;
    Json(status)
}

async fn get_recorder_status(State(state): State<Arc<AppState>>) -> Json<FlightRecorderStatus> {
    let status = state.recorder.status().await;
    Json(status)
}
