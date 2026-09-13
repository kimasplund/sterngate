pub mod dashboard;
pub mod recorder;
pub mod routes;
pub mod state;
pub mod ws;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

pub use recorder::{FlightRecorder, FlightRecorderStatus};
pub use routes::create_router;
pub use state::AppState;

pub async fn run_server(state: Arc<AppState>, port: u16) -> Result<()> {
    let app = create_router(state.clone());
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Sterngate Web Dashboard listening on http://{}", addr);

    // Background periodic telemetry sampler (10 Hz)
    // Runs CAN queries when a flight recording is actively capturing OR WebSocket clients are listening
    let bg_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            if (bg_state.recorder.is_recording() || bg_state.telemetry_tx.receiver_count() > 0)
                && !bg_state.flasher.is_locked().await
            {
                let _ = routes::sample_telemetry(&bg_state).await;
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::json;
    use sterngate_core::{TelemetrySnapshot, VehicleProfile};
    use sterngate_hal::{VehicleInterface, VirtualCanInterface};
    use sterngate_protocol::FlashingWorker;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_flight_recorder_lifecycle() {
        let temp_dir =
            std::env::temp_dir().join(format!("sterngate_test_rec_{}", uuid::Uuid::new_v4()));
        let recorder = FlightRecorder::new(&temp_dir);
        let (tx, rx) = tokio::sync::broadcast::channel(16);

        let status = recorder
            .start(Some("track_test.csv".into()), rx)
            .await
            .unwrap();
        assert!(status.is_recording);
        assert!(recorder.is_recording());

        let snap = TelemetrySnapshot {
            timestamp_ms: 1700000000000,
            battery_voltage: 14.1,
            engine_rpm: Some(2450.0),
            coolant_temp: Some(89.0),
            trans_fluid_temp: Some(80.0),
            rail_pressure: Some(850.0),
            boost_pressure: Some(1550.0),
            tcc_slip_rpm: Some(5.0),
            inj_corr_cyl1: Some(0.12),
            inj_corr_cyl2: Some(-0.08),
            inj_corr_cyl3: Some(-0.15),
            inj_corr_cyl4: Some(0.11),
            parameters: vec![],
        };

        let _ = tx.send(snap);
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let stopped_status = recorder.stop().await;
        assert!(!stopped_status.is_recording);
        assert_eq!(stopped_status.records_count, 1);

        let log_file = temp_dir.join("track_test.csv");
        assert!(log_file.exists());
        let content = std::fs::read_to_string(&log_file).unwrap();
        assert!(content.starts_with("timestamp_ms,battery_voltage,engine_rpm"));
        assert!(content.contains(
            "1700000000000,14.1,2450.0,89.0,80.0,850.0,1550.0,5.0,0.12,-0.08,-0.15,0.11"
        ));

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn test_server_routine_endpoint() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));
        let app = create_router(state);

        let body = json!({
            "module": "EDC16",
            "routine_id_hex": "0xFF01",
            "sub_function": 1
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/routine")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_server_cbf_and_profiles_endpoints() {
        let iface = Box::new(VirtualCanInterface::new());
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        // 1. Test /api/v1/cbf/stats
        let req = Request::builder()
            .uri("/api/v1/cbf/stats")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 2. Test /api/v1/cbf/search?q=VGS
        let req = Request::builder()
            .uri("/api/v1/cbf/search?q=VGS")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. Test /api/v1/cbf/inspect/EGS52
        let req = Request::builder()
            .uri("/api/v1/cbf/inspect/EGS52")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 4. Test /api/v1/profiles
        let req = Request::builder()
            .uri("/api/v1/profiles")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
