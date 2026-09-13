pub mod assets;
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
    async fn test_server_ecu_catalog_and_profiles_endpoints() {
        let iface = Box::new(VirtualCanInterface::new());
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        // 1. Test /api/v1/ecu/stats
        let req = Request::builder()
            .uri("/api/v1/ecu/stats")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 2. Test /api/v1/ecu/search?q=VGS
        let req = Request::builder()
            .uri("/api/v1/ecu/search?q=VGS")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. Test /api/v1/ecu/inspect/EGS52
        let req = Request::builder()
            .uri("/api/v1/ecu/inspect/EGS52")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 4. Test backward-compatible /api/v1/cbf/stats
        let req = Request::builder()
            .uri("/api/v1/cbf/stats")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 5. Test /api/v1/profiles
        let req = Request::builder()
            .uri("/api/v1/profiles")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_static_assets_and_i18n_endpoints() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        // 1. Root index.html
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );

        // 2. CSS asset
        let req = Request::builder()
            .uri("/static/css/style.css")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/css; charset=utf-8"
        );

        // 3. JS asset
        let req = Request::builder()
            .uri("/static/js/i18n.js")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/javascript; charset=utf-8"
        );

        // 4. Locales JSON assets
        for lang in &["en", "de", "sv"] {
            let req = Request::builder()
                .uri(format!("/static/locales/{}.json", lang))
                .body(Body::empty())
                .unwrap();
            let resp = create_router(state.clone()).oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            assert_eq!(
                resp.headers().get("content-type").unwrap(),
                "application/json; charset=utf-8"
            );
        }

        // 5. /api/v1/locales
        let req = Request::builder()
            .uri("/api/v1/locales")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let locales: Vec<String> = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(locales, vec!["en", "de", "sv"]);

        // 6. Multilingual DTC query (/api/v1/dtc?lang=de)
        let req = Request::builder()
            .uri("/api/v1/dtc?lang=de")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 7. Localized routine execution with German description
        let routine_payload = json!({
            "module": "EDC16",
            "routine_id_hex": "0xFF01",
            "sub_function": 1,
            "lang": "de"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/routine")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&routine_payload).unwrap()))
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let msg = resp_json["message"].as_str().unwrap();
        assert!(
            msg.contains("Kraftstoffpumpe") || msg.contains("Entlüftung"),
            "Expected German routine name, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_profile_localization_endpoint() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        // 1. GET /api/v1/profile?lang=de
        let req = Request::builder()
            .uri("/api/v1/profile?lang=de")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let prof_de: VehicleProfile = serde_json::from_slice(&body_bytes).unwrap();
        let tcc_de = prof_de
            .parameters
            .iter()
            .find(|p| p.id == "tcc_slip_rpm")
            .unwrap();
        assert_eq!(tcc_de.name, "Drehzahldifferenz KÜB");

        // 2. GET /api/v1/profile?lang=sv
        let req = Request::builder()
            .uri("/api/v1/profile?lang=sv")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let prof_sv: VehicleProfile = serde_json::from_slice(&body_bytes).unwrap();
        let tcc_sv = prof_sv
            .parameters
            .iter()
            .find(|p| p.id == "tcc_slip_rpm")
            .unwrap();
        assert_eq!(tcc_sv.name, "Momentomvandlarkoppling slirning");
    }

    #[tokio::test]
    async fn test_server_vehicle_scan_garage_and_analytics_endpoints() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        // 1. POST /api/v1/vehicle/scan
        let scan_payload = json!({
            "lang": "en",
            "save_to_garage": true
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vehicle/scan")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&scan_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let report: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let vin = report["vin"].as_str().unwrap();
        assert_eq!(vin, "WDB2112061A892341");
        assert!(report["decoded"]["body_style"]
            .as_str()
            .unwrap()
            .contains("S211"));
        assert_eq!(
            report["decoded"]["model_name"].as_str().unwrap(),
            "E 220 T CDI"
        );

        // 2. GET /api/v1/vehicles
        let req = Request::builder()
            .uri("/api/v1/vehicles")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. GET /api/v1/vehicles/{vin}
        let req = Request::builder()
            .uri(format!("/api/v1/vehicles/{}", vin))
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 4. GET /api/v1/vehicles/{vin}/history
        let req = Request::builder()
            .uri(format!("/api/v1/vehicles/{}/history", vin))
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 5. POST /api/v1/analyze/suspension
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/analyze/suspension")
            .header("Content-Type", "application/json")
            .body(Body::from(b"{}".to_vec()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 6. POST /api/v1/analyze/compare
        let comp_payload = json!({
            "run_a": {
                "duration_seconds": 1800.0,
                "distance_km": 35.0,
                "average_speed_kmh": 70.0,
                "average_consumption_l_per_100km": 7.6,
                "average_rpm": 1950.0,
                "max_boost_hpa": 1450.0,
                "average_rail_pressure_bar": 1150.0,
                "average_tcc_slip_rpm": 38.0,
                "final_coolant_temp_c": 78.0,
                "seconds_to_reach_85c": null
            },
            "run_b": {
                "duration_seconds": 1800.0,
                "distance_km": 35.0,
                "average_speed_kmh": 70.0,
                "average_consumption_l_per_100km": 6.9,
                "average_rpm": 1900.0,
                "max_boost_hpa": 1480.0,
                "average_rail_pressure_bar": 1140.0,
                "average_tcc_slip_rpm": 8.0,
                "final_coolant_temp_c": 88.0,
                "seconds_to_reach_85c": 420.0
            }
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/analyze/compare")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&comp_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 7. POST /api/v1/suspension/compressor/control (Inhibit / Safe mode)
        let ctrl_payload = json!({
            "action": "inhibit",
            "reason": "Test inhibit"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/suspension/compressor/control")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&ctrl_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let ctrl_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(ctrl_res["success"].as_bool().unwrap());

        // 8. GET /api/v1/suspension/compressor/status
        let req = Request::builder()
            .uri("/api/v1/suspension/compressor/status")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let status_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(status_res["is_inhibited"].as_bool().unwrap());

        // 9. GET /api/v1/analyze/cascades
        let req = Request::builder()
            .uri("/api/v1/analyze/cascades")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let cascade_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(cascade_res["total_cascades_checked"], 7);

        // 10. POST /api/v1/analyze/cascades with critical danger input
        let danger_payload = json!({
            "sbc_accumulator_pressure_bar": 48.0,
            "max_cylinder_balance_trim_mm3": 4.2
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/analyze/cascades")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&danger_payload).unwrap()))
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let danger_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(danger_res["overall_severity"], "ImminentDanger");
        assert!(danger_res["alerts"].as_array().unwrap().len() >= 2);
    }
}
