use serde_json::{json, Value};
use sterngate_core::{
    CascadeTelemetryInput, CascadeWatchdog, DriveBenchmark, DriveSummary, SuspensionLeakDetector,
    SuspensionSample,
};
use sterngate_hal::{VehicleInterface, VirtualCanInterface};
use sterngate_protocol::VehicleScanner;

pub async fn handle(name: &str, arguments: &Value) -> Result<Value, String> {
    let mut mock_iface = VirtualCanInterface::new();
    let _ = mock_iface.open().await;

    match name {
        "sterngate_analyze_suspension_leak" => {
            let mut detector = SuspensionLeakDetector::new();
            if let Some(samples) = arguments.get("samples").and_then(|s| s.as_array()) {
                for s in samples {
                    if let Ok(sample) = serde_json::from_value::<SuspensionSample>(s.clone()) {
                        detector.add_sample(sample);
                    }
                }
            } else if arguments.get("left_rear_start_mm").is_some() {
                let start_l = arguments
                    .get("left_rear_start_mm")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(118.0);
                let end_l = arguments
                    .get("left_rear_end_mm")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(start_l);
                let start_r = arguments
                    .get("right_rear_start_mm")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(118.5);
                let end_r = arguments
                    .get("right_rear_end_mm")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(start_r);
                let dur_min = arguments
                    .get("duration_min")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(30.0);
                let comp_run = arguments
                    .get("compressor_run_time_sec")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let comp_active = comp_run > 0.0;

                detector.add_sample(SuspensionSample {
                    timestamp_ms: 1000,
                    left_rear_height_mm: start_l,
                    right_rear_height_mm: start_r,
                    compressor_active: false,
                    compressor_run_duration_s: 0.0,
                    reservoir_pressure_bar: Some(14.2),
                    compressor_temp_c: Some(38.0),
                });
                detector.add_sample(SuspensionSample {
                    timestamp_ms: 1000 + (dur_min * 60_000.0) as u64,
                    left_rear_height_mm: end_l,
                    right_rear_height_mm: end_r,
                    compressor_active: comp_active,
                    compressor_run_duration_s: comp_run,
                    reservoir_pressure_bar: Some(14.0),
                    compressor_temp_c: Some(40.0),
                });
            } else {
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
            Ok(serde_json::to_value(report).unwrap())
        }
        "sterngate_compare_drive_runs" => {
            let (run1, run2, name1, name2) =
                if arguments.get("run_a").is_some() && arguments.get("run_b").is_some() {
                    let r1: DriveSummary =
                        serde_json::from_value(arguments.get("run_a").unwrap().clone())
                            .map_err(|e| format!("Invalid run_a: {}", e))?;
                    let r2: DriveSummary =
                        serde_json::from_value(arguments.get("run_b").unwrap().clone())
                            .map_err(|e| format!("Invalid run_b: {}", e))?;
                    let n1 = arguments
                        .get("name_a")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Run A (Baseline)");
                    let n2 = arguments
                        .get("name_b")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Run B (Modified)");
                    (r1, r2, n1.to_string(), n2.to_string())
                } else if arguments.get("baseline_fuel_consumed_liters").is_some() {
                    let dist_a = arguments
                        .get("baseline_distance_km")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(100.0);
                    let dur_a = arguments
                        .get("baseline_duration_sec")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(3600.0);
                    let fuel_a = arguments
                        .get("baseline_fuel_consumed_liters")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(7.5);
                    let avg_cons_a = (fuel_a / dist_a.max(0.1)) * 100.0;
                    let boost_a = arguments
                        .get("baseline_avg_boost_bar")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.15)
                        * 1000.0;
                    let rail_a = arguments
                        .get("baseline_avg_rail_pressure_bar")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1200.0);

                    let dist_b = arguments
                        .get("target_distance_km")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(100.0);
                    let dur_b = arguments
                        .get("target_duration_sec")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(3600.0);
                    let fuel_b = arguments
                        .get("target_fuel_consumed_liters")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(6.5);
                    let avg_cons_b = (fuel_b / dist_b.max(0.1)) * 100.0;
                    let boost_b = arguments
                        .get("target_avg_boost_bar")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.20)
                        * 1000.0;
                    let rail_b = arguments
                        .get("target_avg_rail_pressure_bar")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1250.0);

                    let r1 = DriveSummary {
                        duration_seconds: dur_a,
                        distance_km: dist_a,
                        average_speed_kmh: (dist_a / (dur_a / 3600.0).max(0.001)),
                        average_consumption_l_per_100km: avg_cons_a,
                        average_rpm: 1950.0,
                        max_boost_hpa: boost_a,
                        average_rail_pressure_bar: rail_a,
                        average_tcc_slip_rpm: 25.0,
                        final_coolant_temp_c: 85.0,
                        seconds_to_reach_85c: None,
                    };
                    let r2 = DriveSummary {
                        duration_seconds: dur_b,
                        distance_km: dist_b,
                        average_speed_kmh: (dist_b / (dur_b / 3600.0).max(0.001)),
                        average_consumption_l_per_100km: avg_cons_b,
                        average_rpm: 1900.0,
                        max_boost_hpa: boost_b,
                        average_rail_pressure_bar: rail_b,
                        average_tcc_slip_rpm: 10.0,
                        final_coolant_temp_c: 88.0,
                        seconds_to_reach_85c: None,
                    };
                    let n1 = arguments
                        .get("baseline_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Baseline");
                    let n2 = arguments
                        .get("target_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Target");
                    (r1, r2, n1.to_string(), n2.to_string())
                } else {
                    let run1 = DriveSummary {
                        duration_seconds: 1800.0,
                        distance_km: 35.0,
                        average_speed_kmh: 70.0,
                        average_consumption_l_per_100km: 7.6,
                        average_rpm: 1950.0,
                        max_boost_hpa: 1450.0,
                        average_rail_pressure_bar: 1150.0,
                        average_tcc_slip_rpm: 38.0,
                        final_coolant_temp_c: 78.0,
                        seconds_to_reach_85c: None,
                    };
                    let run2 = DriveSummary {
                        duration_seconds: 1800.0,
                        distance_km: 35.0,
                        average_speed_kmh: 70.0,
                        average_consumption_l_per_100km: 6.9,
                        average_rpm: 1900.0,
                        max_boost_hpa: 1480.0,
                        average_rail_pressure_bar: 1140.0,
                        average_tcc_slip_rpm: 8.0,
                        final_coolant_temp_c: 88.0,
                        seconds_to_reach_85c: Some(420.0),
                    };
                    (
                        run1,
                        run2,
                        "Baseline".to_string(),
                        "After Service".to_string(),
                    )
                };
            let cmp = DriveBenchmark::compare(&run1, &run2, &name1, &name2);
            Ok(serde_json::to_value(cmp).unwrap())
        }
        "sterngate_protect_compressor" => {
            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("inhibit");
            let reason = arguments
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("AI Agent diagnostic protection request");

            let res = VehicleScanner::control_suspension_compressor(&mut mock_iface, action)
                .await
                .map_err(|e| format!("Failed to execute compressor command: {}", e))?;

            Ok(json!({
                "success": true,
                "action": action,
                "reason": reason,
                "message": res,
                "compressor_relay_status": if action == "inhibit" || action == "workshop" { "DE_ENERGIZED" } else { "NORMAL" },
                "burnout_prevention_active": action == "inhibit" || action == "workshop",
            }))
        }
        "sterngate_control_abc_limiter" => {
            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("dump");

            let res = VehicleScanner::control_abc_safety_limiter(&mut mock_iface, action)
                .await
                .map_err(|e| format!("Failed to execute ABC limiter routine: {}", e))?;

            Ok(json!({
                "success": true,
                "action": action,
                "message": res,
                "abc_system_status": if action == "dump" { "PRESSURE_LIMITED_120BAR" } else if action == "lock" { "STRUT_VALVES_ISOLATED" } else { "ACTIVE_DYNAMIC_CONTROL" },
            }))
        }
        "sterngate_check_cascade_warnings" => {
            let mut input = CascadeTelemetryInput {
                sbc_accumulator_pressure_bar: Some(78.0),
                sbc_pump_per_brake_ratio: Some(0.18),
                sbc_operating_cycles: Some(125_000),
                sbc_max_cycles: Some(300_000),
                max_cylinder_balance_trim_mm3: Some(0.8),
                cylinder_balance_spread_mm3: Some(1.2),
                rail_pressure_bleed_rate_bar_sec: Some(12.0),
                atf_temp_rapid_jump_deg_c: Some(0.5),
                transmission_speed_sensor_jitter: Some(false),
                tcc_slip_rpm: Some(8.0),
                tcc_lockup_commanded: Some(true),
                dpf_diff_pressure_mbar: Some(35.0),
                engine_rpm: Some(750.0),
                distance_since_dpf_regen_km: Some(420.0),
                cam_magnet_oil_detected: Some(false),
                o2_sensor_heater_resistance_drift: Some(false),
                five_volt_ref_bus_dip: Some(false),
                compressor_continuous_run_sec: Some(0.0),
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
                engine_oil_temperature_c: Some(90.0),
                active_dtcs: vec![],
            };

            if let Some(v) = arguments
                .get("sbc_accumulator_pressure_bar")
                .and_then(|v| v.as_f64())
            {
                input.sbc_accumulator_pressure_bar = Some(v);
            }
            if let Some(v) = arguments
                .get("max_cylinder_balance_trim_mm3")
                .and_then(|v| v.as_f64())
            {
                input.max_cylinder_balance_trim_mm3 = Some(v);
            }
            if let Some(v) = arguments.get("tcc_slip_rpm").and_then(|v| v.as_f64()) {
                input.tcc_slip_rpm = Some(v);
            }
            if let Some(v) = arguments
                .get("compressor_continuous_run_sec")
                .and_then(|v| v.as_f64())
            {
                input.compressor_continuous_run_sec = Some(v);
            }
            if let Some(v) = arguments
                .get("suspension_height_drop_rate_mm_h")
                .and_then(|v| v.as_f64())
            {
                input.suspension_height_drop_rate_mm_h = Some(v);
            }
            if let Some(v) = arguments
                .get("abc_pressure_ripple_bar")
                .and_then(|v| v.as_f64())
            {
                input.abc_pressure_ripple_bar = Some(v);
            }
            if let Some(v) = arguments
                .get("esl_unlock_duration_ms")
                .and_then(|v| v.as_f64())
            {
                input.esl_unlock_duration_ms = Some(v);
            }
            if let Some(v) = arguments
                .get("cam_phase_deviation_deg")
                .and_then(|v| v.as_f64())
            {
                input.cam_phase_deviation_deg = Some(v);
            }
            if let Some(v) = arguments
                .get("tcc_slip_oscillation_hz")
                .and_then(|v| v.as_f64())
            {
                input.tcc_slip_oscillation_hz = Some(v);
            }
            if let Some(v) = arguments
                .get("can_sleep_delay_seconds")
                .and_then(|v| v.as_f64())
            {
                input.can_sleep_delay_seconds = Some(v);
            }
            if let Some(v) = arguments
                .get("dynamic_oil_loss_rate_mm_100km")
                .and_then(|v| v.as_f64())
            {
                input.dynamic_oil_loss_rate_mm_100km = Some(v);
            }

            let report = CascadeWatchdog::evaluate(&input);
            Ok(json!(report))
        }
        _ => Err(format!("Unknown tool name: {}", name)),
    }
}
