pub mod assets;
pub mod recorder;
pub mod routes;
pub mod state;
pub mod ws;

use anyhow::Result;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tracing::{info, warn};

pub use recorder::{FlightRecorder, FlightRecorderStatus};
pub use routes::create_router;
pub use state::AppState;

pub async fn run_server(state: Arc<AppState>, bind: IpAddr, port: u16) -> Result<()> {
    let app = create_router(state.clone());
    let addr = SocketAddr::new(bind, port);

    if !bind.is_loopback() {
        warn!(
            "Dashboard bound to {}: the diagnostic API is reachable from the \
             network and has no authentication. Anyone who can reach this port \
             can actuate the vehicle. Bind 127.0.0.1 unless that is intended.",
            bind
        );
    }
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
            battery_voltage: Some(14.1),
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
        assert_eq!(cascade_res["total_cascades_checked"], 13);

        // 10. POST /api/v1/abc/control (ABC Pressure Dump Safe Mode)
        let abc_payload = json!({
            "action": "dump"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/abc/control")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&abc_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let abc_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(abc_res["success"].as_bool().unwrap());

        // 11. POST /api/v1/analyze/cascades with critical danger input
        let danger_payload = json!({
            "sbc_accumulator_pressure_bar": 48.0,
            "max_cylinder_balance_trim_mm3": 4.2,
            "abc_pressure_ripple_bar": 30.0,
            "esl_unlock_duration_ms": 550.0
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
        assert!(danger_res["alerts"].as_array().unwrap().len() >= 4);
    }

    #[tokio::test]
    async fn test_service_and_discovery_and_report_endpoints() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        // 1. POST /api/v1/service/sbc Deactivate
        let sbc_deact = json!({
            "action": "Deactivate"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/service/sbc")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&sbc_deact).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let sbc_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(sbc_res["success"].as_bool().unwrap());
        assert_eq!(sbc_res["status"]["accumulator_pressure_bar"], 0.0);

        // 2. POST /api/v1/service/sbc Reactivate
        let sbc_react = json!({
            "action": "Reactivate"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/service/sbc")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&sbc_react).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. GET /api/v1/service/ima (single cylinder)
        let req = Request::builder()
            .uri("/api/v1/service/ima?cylinder=1")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let ima_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(ima_res["success"].as_bool().unwrap());
        assert_eq!(ima_res["injectors"].as_array().unwrap().len(), 1);

        // 4. POST /api/v1/service/ima (write calibration)
        let ima_write = json!({
            "cylinder": 1,
            "code": "7B8HNA"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/service/ima")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&ima_write).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 5. POST /api/v1/service/suspension
        let susp_act = json!({
            "corner": "RearLeft",
            "action": "Inflate"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/service/suspension")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&susp_act).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 6. POST /api/v1/diag/discover
        let disc_payload = json!({
            "start_id": 2016, // 0x7E0
            "end_id": 2024,   // 0x7E8
            "timeout_ms": 15
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/diag/discover")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&disc_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let disc_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(disc_res["success"].as_bool().unwrap());

        // 7. GET /api/v1/diag/report.html
        let req = Request::builder()
            .uri("/api/v1/diag/report.html?lang=en")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let html_str = String::from_utf8_lossy(&body_bytes);
        assert!(html_str.contains("<!DOCTYPE html>"));
        assert!(html_str.contains("Sterngate"));

        // 8. GET /api/v1/service/routines
        let req = Request::builder()
            .uri("/api/v1/service/routines?q=steering")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let r_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(r_res["total"].as_u64().unwrap() > 0);

        // 9. POST /api/v1/service/routines/execute
        let exec_payload = json!({
            "routine_id": "0x0305",
            "tx_id": 2016,
            "rx_id": 2024
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/service/routines/execute")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&exec_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 10. GET /api/v1/coding/dids
        let req = Request::builder()
            .uri("/api/v1/coding/dids?q=vin")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let c_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(c_res["total"].as_u64().unwrap() > 0);

        // 11. POST /api/v1/coding/revin
        let revin_payload = json!({
            "target_ecu": "CR4",
            "new_vin": "WDB2112061A999888"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/coding/revin")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&revin_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let revin_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(revin_res["success"].as_bool().unwrap());
        assert_eq!(revin_res["new_vin"].as_str().unwrap(), "WDB2112061A999888");
    }

    #[tokio::test]
    async fn test_guided_workflows_endpoints() {
        let mut sim = Box::new(VirtualCanInterface::new());
        let _ = sim.open().await;
        let profile = VehicleProfile::load_from_file(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../profiles/mercedes/w211_om646_edc16.json"),
        )
        .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(sim, profile, flasher));

        // 1. GET /api/v1/workflows
        let req = Request::builder()
            .uri("/api/v1/workflows")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let workflows: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(workflows.as_array().unwrap().len() >= 4);

        // 2. POST /api/v1/workflow/adblue-reset
        let adblue_payload = json!({
            "vin": "WDB2112061A999888",
            "ecu_tx": 2016, // 0x7E0
            "ecu_rx": 2024  // 0x7E8
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/workflow/adblue-reset")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&adblue_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let adblue_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(adblue_res["success"].as_bool().unwrap());
        assert!(adblue_res["countdown_reset"].as_bool().unwrap());

        // 3. POST /api/v1/workflow/eco-start-stop
        let eco_payload = json!({
            "mode": "remember",
            "vin": "WDB2112061A999888"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/workflow/eco-start-stop")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&eco_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let eco_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(eco_res["success"].as_bool().unwrap());

        // 4. POST /api/v1/workflow/egr-optimize
        let egr_payload = json!({
            "vin": "WDB2112061A999888"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/workflow/egr-optimize")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&egr_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let egr_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(egr_res["success"].as_bool().unwrap());
        assert_eq!(egr_res["air_mass_offset_mg"].as_f64().unwrap(), 40.0);

        // 5. POST /api/v1/workflow/vmax
        let vmax_payload = json!({
            "speed_limit_kmh": 250,
            "vin": "WDB2112061A999888"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/workflow/vmax")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&vmax_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let vmax_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(vmax_res["success"].as_bool().unwrap());
        assert_eq!(vmax_res["speed_limit_kmh"].as_u64().unwrap(), 250);

        // 6. POST /api/v1/workflow/seatbelt-chime
        let seatbelt_payload = json!({
            "acoustic_enabled": false,
            "vin": "WDB2112061A999888"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/workflow/seatbelt-chime")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&seatbelt_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let seatbelt_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(seatbelt_res["success"].as_bool().unwrap());
        assert!(!seatbelt_res["acoustic_chime_enabled"].as_bool().unwrap());

        // 7. POST /api/v1/workflow/tank-liters
        let tank_payload = json!({
            "enabled": true,
            "vin": "WDB2112061A999888"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/workflow/tank-liters")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&tank_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let tank_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(tank_res["success"].as_bool().unwrap());
        assert!(tank_res["exact_liters_display_enabled"].as_bool().unwrap());

        // 8. POST /api/v1/workflow/cornering-lights
        let cornering_payload = json!({
            "enabled": true,
            "vin": "WDB2112061A999888"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/workflow/cornering-lights")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&cornering_payload).unwrap()))
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let cornering_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(cornering_res["success"].as_bool().unwrap());
        assert!(cornering_res["cornering_lights_enabled"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_firmware_vault_endpoints() {
        let mut sim = Box::new(VirtualCanInterface::new());
        let _ = sim.open().await;
        let profile = VehicleProfile::load_from_file(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../profiles/mercedes/w211_om646_edc16.json"),
        )
        .unwrap();
        let flasher = Arc::new(FlashingWorker::new());

        let temp_dir =
            std::env::temp_dir().join(format!("sterngate_vault_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let bin_path = temp_dir.join("W211_OM646_Stage1.bin");
        let mut rom = vec![0xEA; 4096];
        rom[64..74].copy_from_slice(b"0281012224");
        rom[128..138].copy_from_slice(b"1037372332");
        std::fs::write(&bin_path, &rom).unwrap();

        // The vault is confined to a configured root; point it at the fixture.
        let state =
            Arc::new(AppState::new(sim, profile, flasher).with_vault_root(Some(temp_dir.clone())));

        // 1. GET /api/v1/vault/scan - a blank path scans the whole vault
        let scan_url = "/api/v1/vault/scan?path=&hw_id=0281012224&sw_id=1037365000";
        let req = Request::builder()
            .uri(scan_url)
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let scan_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(scan_res["success"].as_bool().unwrap());
        assert_eq!(scan_res["total_files"].as_u64().unwrap(), 1);
        assert!(scan_res["recommendation"].is_object());

        // 2. Paths outside the vault root are refused however they are spelled,
        // so a caller can never reach arbitrary files on the host.
        for escape in ["../../../etc/hostname", "/etc/hostname"] {
            let payload = json!({ "file_path": escape, "measured_voltage": 13.4 });
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/vault/stage")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap();
            let resp = create_router(state.clone()).oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "vault escape was allowed: {escape}"
            );
        }

        let escape_scan = "/api/v1/vault/scan?path=../../../etc";
        let req = Request::builder()
            .uri(escape_scan)
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // 3. POST /api/v1/vault/stage without a measured voltage is refused:
        // staging begins a flash, and the >= 12.5 V interlock means nothing if
        // the server supplies the reading itself.
        let unmeasured_payload = json!({
            "file_path": bin_path.to_str().unwrap()
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vault/stage")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&unmeasured_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // 3. POST /api/v1/vault/stage with a measured voltage proceeds
        let stage_payload = json!({
            "file_path": bin_path.to_str().unwrap(),
            "measured_voltage": 13.4
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vault/stage")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&stage_payload).unwrap()))
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let stage_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(stage_res["success"].as_bool().unwrap());
        assert_eq!(
            stage_res["manifest"]["expected_hw_id"].as_str().unwrap(),
            "0281012224"
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_flash_stage_refuses_without_measured_voltage() {
        let mut sim = Box::new(VirtualCanInterface::new());
        let _ = sim.open().await;
        let profile = VehicleProfile::load_from_file(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../profiles/mercedes/w211_om646_edc16.json"),
        )
        .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(sim, profile, flasher));

        let payload = json!({
            "manifest": {
                "target_module": "EDC16",
                "expected_hw_id": "0281012224",
                "expected_sw_id": "1037372332",
                "sha256_checksum": "",
                "crc32_checksum": 0,
                "flash_start_address": 262144,
                "flash_length": 4096,
                "block_size": 4096
            },
            "rom_base64": "dummy_rom_data"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/flash/stage")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&payload).unwrap()))
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_rom_inspection_and_profile_selection_endpoints() {
        use base64::Engine as _;

        let mut sim = Box::new(VirtualCanInterface::new());
        let _ = sim.open().await;
        let profile = VehicleProfile::load_from_file(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../profiles/mercedes/w211_om646_edc16.json"),
        )
        .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(sim, profile, flasher));

        // 1. POST /api/v1/flash/inspect-rom with matching demo binary
        let mut rom = vec![0xEA; 4096];
        let hw = b"0281012224";
        let sw = b"1037372332";
        let oem = b"A 646 150 08 79";
        rom[64..64 + hw.len()].copy_from_slice(hw);
        rom[128..128 + sw.len()].copy_from_slice(sw);
        rom[256..256 + oem.len()].copy_from_slice(oem);
        let b64_rom = base64::engine::general_purpose::STANDARD.encode(&rom);

        let inspect_payload = json!({
            "rom_base64": b64_rom
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/flash/inspect-rom")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&inspect_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let inspect_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(inspect_res["success"].as_bool().unwrap());
        let report = &inspect_res["report"];
        assert!(report["can_flash"].as_bool().unwrap());
        assert_eq!(
            report["signatures"]["bosch_hw_id"].as_str().unwrap(),
            "0281012224"
        );

        // 2. POST /api/v1/profile/select
        let select_payload = json!({
            "name": "w211_om648_edc16"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/profile/select")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&select_payload).unwrap()))
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let select_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(select_res["success"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_community_mods_endpoints() {
        let mut sim = Box::new(VirtualCanInterface::new());
        let _ = sim.open().await;
        let profile = VehicleProfile::load_from_file(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../profiles/mercedes/w211_om646_edc16.json"),
        )
        .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(sim, profile, flasher));

        // 1. POST /api/v1/mods/create
        let create_payload = json!({
            "name": "W211 Top Speed 300",
            "author": "TunerKim",
            "description": "Increases speed limiter to 300 km/h",
            "category": "performance",
            "risk_level": "moderate",
            "chassis": ["W211", "S211"],
            "ecu_name": "EDC16",
            "tx_id": 2016,
            "rx_id": 2024,
            "min_voltage": 12.0,
            "did": 272, // 0x0110
            "data_hex": "012C",
            "action_description": "Set speed governor to 300 km/h"
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/mods/create")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&create_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let create_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(create_res["success"].as_bool().unwrap());
        let armored_text = create_res["armored_text"].as_str().unwrap().to_string();
        assert!(armored_text.contains("BEGIN STERNGATE COMMUNITY MOD"));

        // 2. POST /api/v1/mods/inspect
        let inspect_payload = json!({
            "content": armored_text,
            "vin": "WDB2112061A000001",
            "battery_voltage": 12.8
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/mods/inspect")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&inspect_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let inspect_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(inspect_res["success"].as_bool().unwrap());
        assert!(inspect_res["validation"]["is_valid"].as_bool().unwrap());
        assert!(inspect_res["validation"]["matched_vehicle"]
            .as_bool()
            .unwrap());

        // 3. POST /api/v1/mods/apply
        let apply_payload = json!({
            "content": armored_text,
            "vin": "WDB2112061A000001",
            "battery_voltage": 12.8
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/mods/apply")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&apply_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let apply_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(apply_res["success"].as_bool().unwrap());
        assert_eq!(apply_res["report"]["steps_completed"].as_u64().unwrap(), 1);

        // 4. GET /api/v1/mods/library
        let req = Request::builder()
            .uri("/api/v1/mods/library")
            .body(Body::empty())
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_mods_apply_rejects_force_field() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        let payload = json!({
            "content": "{}",
            "vin": "WDB2112061A000001",
            "battery_voltage": 12.8,
            "force": true
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/mods/apply")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&payload).unwrap()))
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        // axum maps serde data errors (unknown field) to 422.
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_server_tuning_endpoints() {
        use base64::Engine;

        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        // Create synthetic 2MB EDC16 ROM
        let mut rom = vec![0xFF; 0x200000];
        let hw_str = b"0281012238";
        rom[0x1C0020..0x1C0020 + hw_str.len()].copy_from_slice(hw_str);
        let sw_str = b"1037386780";
        rom[0x1C0040..0x1C0040 + sw_str.len()].copy_from_slice(sw_str);
        let svbl_bytes = 2350u16.to_be_bytes();
        rom[0x1C2000] = svbl_bytes[0];
        rom[0x1C2001] = svbl_bytes[1];
        rom[0x1C1FFE] = 0x00;
        rom[0x1C1FFF] = 0x00;
        rom[0x1C2002] = 0x00;
        rom[0x1C2003] = 0x00;

        let rom_b64 = base64::engine::general_purpose::STANDARD.encode(&rom);

        // 1. POST /api/v1/tuning/scan
        let scan_payload = json!({
            "rom_base64": rom_b64
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tuning/scan")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&scan_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let scan_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(scan_res["success"].as_bool().unwrap());
        assert!(scan_res["map_count"].as_u64().unwrap() > 0);

        // 2. POST /api/v1/tuning/stage1 — refuses: no map in this synthetic ROM is rom-backed
        let stage1_payload = json!({
            "rom_base64": rom_b64,
            "chassis": "W211 E280 CDI",
            "ecu_name": "EDC16CP31",
            "author": "TunerKim"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tuning/stage1")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&stage1_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let stage1_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(!stage1_res["success"].as_bool().unwrap());
        assert!(stage1_res["error"]
            .as_str()
            .unwrap()
            .contains("Torque Limiter"));

        // 3. POST /api/v1/tuning/stage2 — refuses for the same reason
        let stage2_payload = json!({
            "rom_base64": rom_b64,
            "chassis": "W211 E280 CDI",
            "ecu_name": "EDC16CP31"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tuning/stage2")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&stage2_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let stage2_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(!stage2_res["success"].as_bool().unwrap());

        // 3b. POST /api/v1/tuning/dtc/kill — unsupported until the detector rebuild
        let dtc_payload = json!({ "rom_base64": rom_b64, "p_codes": ["P0401"] });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tuning/dtc/kill")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&dtc_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        // 4. POST /api/v1/tuning/checksum/verify
        let chk_payload = json!({
            "rom_base64": rom_b64
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tuning/checksum/verify")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&chk_payload).unwrap()))
            .unwrap();
        let resp = create_router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 5. POST /api/v1/tuning/checksum/fix
        let fix_payload = json!({
            "rom_base64": rom_b64
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tuning/checksum/fix")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&fix_payload).unwrap()))
            .unwrap();
        let resp = create_router(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let fix_res: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(fix_res["success"].as_bool().unwrap());
        assert!(fix_res["report"]["is_valid"].as_bool().unwrap());
        assert!(fix_res["fixed_base64"].is_string());
    }

    #[tokio::test]
    async fn test_mods_apply_requires_vin() {
        let mut iface = Box::new(VirtualCanInterface::new());
        let _ = iface.open().await;
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        let flasher = Arc::new(FlashingWorker::new());
        let state = Arc::new(AppState::new(iface, profile, flasher));

        for payload in [
            json!({ "content": "{}", "battery_voltage": 12.8 }),
            json!({ "content": "{}", "vin": "   ", "battery_voltage": 12.8 }),
        ] {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/mods/apply")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap();
            let resp = create_router(state.clone()).oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(v["error"].as_str().unwrap().contains("VIN"));
        }
    }
}
