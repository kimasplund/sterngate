pub mod prompts;
pub mod resources;
pub mod server;
pub mod tools;

pub use server::McpServer;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_mcp_tool_list() {
        let tools = tools::get_tools_list();
        let array = tools.as_array().expect("Tools must be an array");
        assert!(array.len() >= 6);

        let names: Vec<&str> = array
            .iter()
            .map(|t| t.get("name").unwrap().as_str().unwrap())
            .collect();
        assert!(names.contains(&"sterngate_list_interfaces"));
        assert!(names.contains(&"sterngate_read_telemetry"));
        assert!(names.contains(&"sterngate_read_dtc"));
        assert!(names.contains(&"sterngate_clear_dtc"));
        assert!(names.contains(&"sterngate_verify_flash_staging"));
    }

    #[tokio::test]
    async fn test_mcp_read_telemetry_tool() {
        let res = tools::handle_tool_call("sterngate_read_telemetry", &json!({}))
            .await
            .unwrap();
        assert_eq!(res.get("battery_voltage").unwrap().as_f64().unwrap(), 13.8);
        assert_eq!(res.get("coolant_temp").unwrap().as_f64().unwrap(), 88.0);
        assert_eq!(res.get("trans_fluid_temp").unwrap().as_f64().unwrap(), 80.0);
    }

    #[tokio::test]
    async fn test_mcp_read_dtc_tool() {
        let res = tools::handle_tool_call("sterngate_read_dtc", &json!({"module": "EDC16"}))
            .await
            .unwrap();
        let dtcs = res.get("dtcs").unwrap().as_array().unwrap();
        assert_eq!(dtcs.len(), 1);
        assert_eq!(dtcs[0].get("code").unwrap().as_str().unwrap(), "P0100");
    }

    #[tokio::test]
    async fn test_mcp_resources() {
        let list = resources::get_resources_list();
        assert!(list.as_array().unwrap().len() >= 2);

        let res = resources::read_resource("sterngate://profile/w211_om646").unwrap();
        assert_eq!(
            res.get("profile").unwrap().as_str().unwrap(),
            "mercedes_w211_om646_edc16"
        );

        let cat_res = resources::read_resource("sterngate://ecu/catalog").unwrap();
        assert_eq!(cat_res.get("unique_ecus").unwrap().as_u64().unwrap(), 990);
    }

    #[tokio::test]
    async fn test_mcp_trigger_routine_tool() {
        let res = tools::handle_tool_call(
            "sterngate_trigger_routine",
            &json!({"module": "EDC16", "routine_id": "0xFF01", "sub_function": 1}),
        )
        .await
        .unwrap();
        assert!(res.get("success").unwrap().as_bool().unwrap());
        assert_eq!(res.get("routine_id").unwrap().as_str().unwrap(), "0xFF01");
        assert_eq!(
            res.get("routine_name").unwrap().as_str().unwrap(),
            "Fuel Pump Prime & Rail Bleed"
        );
    }

    #[tokio::test]
    async fn test_mcp_control_flight_recorder_tool() {
        let start_res = tools::handle_tool_call(
            "sterngate_control_flight_recorder",
            &json!({"action": "start", "filename": "track_test.csv"}),
        )
        .await
        .unwrap();
        assert!(start_res.get("is_recording").unwrap().as_bool().unwrap());

        let stop_res = tools::handle_tool_call(
            "sterngate_control_flight_recorder",
            &json!({"action": "stop"}),
        )
        .await
        .unwrap();
        assert!(!stop_res.get("is_recording").unwrap().as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_mcp_search_ecu_catalog_tool() {
        let res =
            tools::handle_tool_call("sterngate_search_ecu_catalog", &json!({"query": "EGS52"}))
                .await
                .unwrap();
        let matches = res.get("total_matches").unwrap().as_u64().unwrap();
        assert!(matches >= 1);
        let results = res.get("results").unwrap().as_array().unwrap();
        assert_eq!(
            results[0].get("ecu_name").unwrap().as_str().unwrap(),
            "EGS52"
        );
    }

    #[tokio::test]
    async fn test_mcp_multilingual_dtc_and_routine_tools() {
        // German DTC
        let res_de = tools::handle_tool_call(
            "sterngate_read_dtc",
            &json!({"module": "EDC16", "lang": "de"}),
        )
        .await
        .unwrap();
        let dtcs_de = res_de.get("dtcs").unwrap().as_array().unwrap();
        assert!(dtcs_de[0]
            .get("description")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("Luftmassenmesser"));

        // German Routine
        let res_routine_de = tools::handle_tool_call(
            "sterngate_trigger_routine",
            &json!({"module": "EDC16", "routine_id": "0xFF01", "lang": "de"}),
        )
        .await
        .unwrap();
        assert!(res_routine_de
            .get("routine_name")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("Kraftstoffpumpe"));

        // German Parameter
        let res_param_de = tools::handle_tool_call(
            "sterngate_read_parameter",
            &json!({"parameter": "trans_fluid_temp", "lang": "de"}),
        )
        .await
        .unwrap();
        assert_eq!(
            res_param_de.get("name").unwrap().as_str().unwrap(),
            "Getriebeöltemperatur"
        );

        // Swedish Parameter
        let res_param_sv = tools::handle_tool_call(
            "sterngate_read_parameter",
            &json!({"parameter": "tcc_slip_rpm", "lang": "sv"}),
        )
        .await
        .unwrap();
        assert_eq!(
            res_param_sv.get("name").unwrap().as_str().unwrap(),
            "Momentomvandlarkoppling slirning"
        );
    }

    #[tokio::test]
    async fn test_mcp_inspect_ecu_definition_and_list_locales() {
        // Inspect EGS52
        let ecu_res =
            tools::handle_tool_call("sterngate_inspect_ecu_definition", &json!({"ecu": "EGS52"}))
                .await
                .unwrap();
        assert_eq!(ecu_res.get("ecu_name").unwrap().as_str().unwrap(), "EGS52");
        let canonical = ecu_res.get("canonical_version").unwrap();
        assert!(canonical
            .get("tx_id")
            .unwrap()
            .as_str()
            .unwrap()
            .eq_ignore_ascii_case("0x7E1"));

        // List Locales
        let loc_res = tools::handle_tool_call("sterngate_list_locales", &json!({}))
            .await
            .unwrap();
        let locales = loc_res.get("locales").unwrap().as_array().unwrap();
        assert_eq!(locales.len(), 3);

        // List Profiles
        let prof_res = tools::handle_tool_call("sterngate_list_profiles", &json!({}))
            .await
            .unwrap();
        let count = prof_res.get("count").unwrap().as_u64().unwrap();
        assert!(count >= 1);
    }

    #[tokio::test]
    async fn test_mcp_new_resources() {
        let loc = resources::read_resource("sterngate://locales").unwrap();
        assert!(loc.get("locales").is_some());

        let stats = resources::read_resource("sterngate://cbf/stats").unwrap();
        assert!(stats.get("total_cbf_files").is_some() || stats.get("total_ecus").is_some());

        let garage_res = resources::read_resource("sterngate://garage/vehicles").unwrap();
        assert!(garage_res.get("vehicles").is_some());
    }

    #[tokio::test]
    async fn test_mcp_scan_vehicle_and_garage_tools() {
        let scan_res = tools::handle_tool_call(
            "sterngate_scan_vehicle",
            &json!({"save_vehicle": true, "lang": "en"}),
        )
        .await
        .unwrap();

        assert_eq!(
            scan_res.get("vin").unwrap().as_str().unwrap(),
            "WDB2112061A892341"
        );
        assert!(scan_res.get("module_results").is_some());
        assert!(
            scan_res
                .get("total_modules_probed")
                .unwrap()
                .as_u64()
                .unwrap()
                >= 4
        );

        let list_res = tools::handle_tool_call("sterngate_list_vehicles", &json!({}))
            .await
            .unwrap();
        let vehicles = list_res.get("vehicles").unwrap().as_array().unwrap();
        assert!(!vehicles.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_analytics_tools() {
        // Suspension leak analysis
        let susp_res = tools::handle_tool_call(
            "sterngate_analyze_suspension_leak",
            &json!({
                "duration_min": 30.0,
                "left_rear_start_mm": 375.0,
                "left_rear_end_mm": 362.0,
                "right_rear_start_mm": 374.0,
                "right_rear_end_mm": 373.0,
                "compressor_run_time_sec": 55.0,
                "compressor_duty_cycle_pct": 28.0
            }),
        )
        .await
        .unwrap();

        assert_eq!(
            susp_res.get("status").unwrap().as_str().unwrap(),
            "CriticalLeak"
        );
        assert!(
            susp_res
                .get("recommendations")
                .unwrap()
                .as_array()
                .unwrap()
                .len()
                >= 2
        );

        // Drive run comparison
        let comp_res = tools::handle_tool_call(
            "sterngate_compare_drive_runs",
            &json!({
                "baseline_name": "Stock Map",
                "baseline_distance_km": 100.0,
                "baseline_duration_sec": 3600.0,
                "baseline_fuel_consumed_liters": 6.8,
                "baseline_avg_boost_bar": 1.15,
                "baseline_avg_rail_pressure_bar": 1250.0,
                "target_name": "Stage 1 Eco",
                "target_distance_km": 100.0,
                "target_duration_sec": 3600.0,
                "target_fuel_consumed_liters": 6.2,
                "target_avg_boost_bar": 1.20,
                "target_avg_rail_pressure_bar": 1300.0
            }),
        )
        .await
        .unwrap();

        assert!(comp_res
            .get("verdict")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("Beneficial"));
    }

    #[tokio::test]
    async fn test_mcp_protect_compressor_tool() {
        // 1. Inhibit (Burnout safe mode)
        let res_inhibit = tools::handle_tool_call(
            "sterngate_protect_compressor",
            &json!({"action": "inhibit", "reason": "Air leak detected"}),
        )
        .await
        .unwrap();

        assert!(res_inhibit.get("success").unwrap().as_bool().unwrap());
        assert_eq!(
            res_inhibit
                .get("compressor_relay_status")
                .unwrap()
                .as_str()
                .unwrap(),
            "DE_ENERGIZED"
        );
        assert!(res_inhibit
            .get("burnout_prevention_active")
            .unwrap()
            .as_bool()
            .unwrap());

        // 2. Workshop / Transport mode
        let res_ws = tools::handle_tool_call(
            "sterngate_protect_compressor",
            &json!({"action": "workshop"}),
        )
        .await
        .unwrap();
        assert!(res_ws.get("success").unwrap().as_bool().unwrap());

        // 3. Restore normal
        let res_restore = tools::handle_tool_call(
            "sterngate_protect_compressor",
            &json!({"action": "restore"}),
        )
        .await
        .unwrap();
        assert!(res_restore.get("success").unwrap().as_bool().unwrap());
        assert_eq!(
            res_restore
                .get("compressor_relay_status")
                .unwrap()
                .as_str()
                .unwrap(),
            "NORMAL"
        );
    }

    #[tokio::test]
    async fn test_mcp_check_cascade_warnings_tool() {
        // 1. Healthy check
        let res_healthy = tools::handle_tool_call(
            "sterngate_check_cascade_warnings",
            &json!({
                "sbc_accumulator_pressure_bar": 78.0,
                "max_cylinder_balance_trim_mm3": 0.8
            }),
        )
        .await
        .unwrap();

        assert_eq!(
            res_healthy
                .get("overall_severity")
                .unwrap()
                .as_str()
                .unwrap(),
            "Normal"
        );
        assert_eq!(
            res_healthy
                .get("total_cascades_checked")
                .unwrap()
                .as_u64()
                .unwrap(),
            13
        );

        // 2. Imminent danger check
        let res_danger = tools::handle_tool_call(
            "sterngate_check_cascade_warnings",
            &json!({
                "sbc_accumulator_pressure_bar": 45.0,
                "max_cylinder_balance_trim_mm3": 4.5,
                "compressor_continuous_run_sec": 48.0,
                "abc_pressure_ripple_bar": 28.0,
                "esl_unlock_duration_ms": 600.0
            }),
        )
        .await
        .unwrap();

        assert_eq!(
            res_danger
                .get("overall_severity")
                .unwrap()
                .as_str()
                .unwrap(),
            "ImminentDanger"
        );
        let alerts = res_danger.get("alerts").unwrap().as_array().unwrap();
        assert!(alerts.len() >= 5);

        // 3. Resource check
        let catalog_res = resources::read_resource("sterngate://cascades/catalog").unwrap();
        assert_eq!(
            catalog_res.get("total_cascades").unwrap().as_u64().unwrap(),
            13
        );

        // 4. Test sterngate_control_abc_limiter
        let abc_dump_res =
            tools::handle_tool_call("sterngate_control_abc_limiter", &json!({"action": "dump"}))
                .await
                .unwrap();
        assert!(abc_dump_res.get("success").unwrap().as_bool().unwrap());
        assert_eq!(
            abc_dump_res
                .get("abc_system_status")
                .unwrap()
                .as_str()
                .unwrap(),
            "PRESSURE_LIMITED_120BAR"
        );
    }
}
