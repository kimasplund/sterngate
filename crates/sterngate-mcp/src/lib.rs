#![recursion_limit = "256"]

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
        assert!(cat_res.get("unique_ecus").unwrap().as_u64().unwrap() >= 1340);
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

    #[tokio::test]
    async fn test_mcp_discovery_service_flash_and_report_tools() {
        // 1. sterngate_discover_ecus
        let disc_res = tools::handle_tool_call(
            "sterngate_discover_ecus",
            &json!({
                "start_id": 2016, // 0x7E0
                "end_id": 2024,   // 0x7E8
                "timeout_ms": 15
            }),
        )
        .await
        .unwrap();
        assert!(disc_res.get("success").unwrap().as_bool().unwrap());
        assert!(disc_res.get("discovered_count").unwrap().as_u64().unwrap() >= 1);

        // 2. sterngate_service_routine: sbc_deactivate
        let sbc_deact = tools::handle_tool_call(
            "sterngate_service_routine",
            &json!({"routine": "sbc_deactivate"}),
        )
        .await
        .unwrap();
        assert!(sbc_deact.get("success").unwrap().as_bool().unwrap());
        assert_eq!(
            sbc_deact["status"]["accumulator_pressure_bar"]
                .as_f64()
                .unwrap(),
            0.0
        );

        // 3. sterngate_service_routine: sbc_reactivate
        let sbc_react = tools::handle_tool_call(
            "sterngate_service_routine",
            &json!({"routine": "sbc_reactivate"}),
        )
        .await
        .unwrap();
        assert!(sbc_react.get("success").unwrap().as_bool().unwrap());
        assert!(
            sbc_react["status"]["accumulator_pressure_bar"]
                .as_f64()
                .unwrap()
                > 100.0
        );

        // 4. sterngate_service_routine: read_ima
        let read_ima = tools::handle_tool_call(
            "sterngate_service_routine",
            &json!({"routine": "read_ima", "cylinder": 1}),
        )
        .await
        .unwrap();
        assert!(read_ima.get("success").unwrap().as_bool().unwrap());
        assert_eq!(read_ima["injector"]["cylinder"].as_u64().unwrap(), 1);

        // 5. sterngate_service_routine: write_ima
        let write_ima = tools::handle_tool_call(
            "sterngate_service_routine",
            &json!({
                "routine": "write_ima",
                "cylinder": 1,
                "code": "7B8HNA"
            }),
        )
        .await
        .unwrap();
        assert!(write_ima.get("success").unwrap().as_bool().unwrap());
        assert!(write_ima.get("git_recorded").unwrap().as_bool().unwrap());

        // 6. sterngate_service_routine: suspension_corner
        let susp = tools::handle_tool_call(
            "sterngate_service_routine",
            &json!({
                "routine": "suspension_corner",
                "corner": "RearLeft",
                "action": "inflate"
            }),
        )
        .await
        .unwrap();
        assert!(susp.get("success").unwrap().as_bool().unwrap());

        // 7. sterngate_flash_ecu: voltage interlock failure (<12.5V)
        let flash_fail = tools::handle_tool_call(
            "sterngate_flash_ecu",
            &json!({
                "target_module": "EDC16",
                "battery_voltage": 11.9
            }),
        )
        .await;
        assert!(flash_fail.is_err());
        assert!(flash_fail
            .unwrap_err()
            .contains("FLASH INTERLOCK VIOLATION"));

        // 8. sterngate_flash_ecu: dry_run
        let flash_dry = tools::handle_tool_call(
            "sterngate_flash_ecu",
            &json!({
                "target_module": "EDC16",
                "battery_voltage": 13.8,
                "dry_run": true
            }),
        )
        .await
        .unwrap();
        assert!(flash_dry
            .get("preflight_passed")
            .unwrap()
            .as_bool()
            .unwrap());

        // 9. sterngate_flash_ecu: full execution
        let flash_full = tools::handle_tool_call(
            "sterngate_flash_ecu",
            &json!({
                "target_module": "EDC16",
                "battery_voltage": 13.8
            }),
        )
        .await
        .unwrap();
        assert!(flash_full.get("success").unwrap().as_bool().unwrap());

        // 10. sterngate_export_report
        let rep_res = tools::handle_tool_call("sterngate_export_report", &json!({"lang": "en"}))
            .await
            .unwrap();
        assert!(rep_res.get("success").unwrap().as_bool().unwrap());
        assert!(rep_res["html_size_bytes"].as_u64().unwrap() > 500);

        // 11. Resource: sterngate://service/routines
        let serv_res = resources::read_resource("sterngate://service/routines").unwrap();
        assert_eq!(
            serv_res.get("routines").unwrap().as_array().unwrap().len(),
            4
        );
    }

    #[tokio::test]
    async fn test_mcp_workflows_vault_and_importer_tools() {
        // 1. sterngate_guided_workflow: vmax
        let vmax_res = tools::handle_tool_call(
            "sterngate_guided_workflow",
            &json!({
                "workflow": "vmax",
                "speed_limit_kmh": 250,
                "vin": "WDB2112061A999888"
            }),
        )
        .await
        .unwrap();
        assert!(vmax_res["success"].as_bool().unwrap());
        assert_eq!(vmax_res["speed_limit_kmh"].as_u64().unwrap(), 250);

        // 2. sterngate_guided_workflow: seatbelt_chime
        let seatbelt_res = tools::handle_tool_call(
            "sterngate_guided_workflow",
            &json!({
                "workflow": "seatbelt_chime",
                "enabled": false,
                "vin": "WDB2112061A999888"
            }),
        )
        .await
        .unwrap();
        assert!(seatbelt_res["success"].as_bool().unwrap());
        assert!(!seatbelt_res["acoustic_chime_enabled"].as_bool().unwrap());

        // 3. sterngate_guided_workflow: tank_liters
        let tank_res = tools::handle_tool_call(
            "sterngate_guided_workflow",
            &json!({
                "workflow": "tank_liters",
                "enabled": true,
                "vin": "WDB2112061A999888"
            }),
        )
        .await
        .unwrap();
        assert!(tank_res["success"].as_bool().unwrap());
        assert!(tank_res["exact_liters_display_enabled"].as_bool().unwrap());

        // 4. sterngate_guided_workflow: cornering_lights
        let corner_res = tools::handle_tool_call(
            "sterngate_guided_workflow",
            &json!({
                "workflow": "cornering_lights",
                "enabled": true,
                "vin": "WDB2112061A999888"
            }),
        )
        .await
        .unwrap();
        assert!(corner_res["success"].as_bool().unwrap());
        assert!(corner_res["cornering_lights_enabled"].as_bool().unwrap());

        // 5. sterngate_vault_scan
        let vault_res = tools::handle_tool_call(
            "sterngate_vault_scan",
            &json!({
                "path": "profiles",
                "hw_id": "0281012224",
                "sw_id": "1037365000"
            }),
        )
        .await
        .unwrap();
        assert!(vault_res["success"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_mcp_community_mods_suite() {
        // 1. Create a community mod via MCP
        let create_args = json!({
            "name": "AMG Cluster Logo & Sweep",
            "author": "BenzTuner_MCP",
            "description": "Enables AMG logo on instrument cluster and needle sweep on startup",
            "chassis": "W211",
            "ecu": "IC_211",
            "did": "0x0201",
            "data": "00",
            "bitmask": "01",
            "category": "comfort",
            "risk_level": "low",
            "min_voltage": 12.2,
            "instructions": "Cycle key after programming."
        });

        let create_res = tools::handle_tool_call("sterngate_create_community_mod", &create_args)
            .await
            .unwrap();

        assert!(create_res["success"].as_bool().unwrap());
        let mod_id = create_res["mod_id"].as_str().unwrap();
        assert!(mod_id.starts_with("amg_cluster_logo__sweep"));
        let armor = create_res["armored_text"].as_str().unwrap();
        assert!(armor.contains("BEGIN STERNGATE COMMUNITY MOD"));

        // 2. Inspect the created mod via MCP
        let inspect_args = json!({
            "mod_content": armor,
            "vin": "WDB2110061A123456",
            "battery_voltage": 12.6
        });

        let inspect_res = tools::handle_tool_call("sterngate_inspect_community_mod", &inspect_args)
            .await
            .unwrap();

        assert!(inspect_res["success"].as_bool().unwrap());
        assert!(inspect_res["integrity"]["is_valid"].as_bool().unwrap());
        assert!(inspect_res["compatibility"]["matched_vehicle"]
            .as_bool()
            .unwrap());

        // 3. Apply the mod via MCP
        let apply_args = json!({
            "mod_content": armor,
            "vin": "WDB2110061A123456",
            "battery_voltage": 12.8
        });

        let apply_res = tools::handle_tool_call("sterngate_apply_community_mod", &apply_args)
            .await
            .unwrap();

        assert!(apply_res["success"].as_bool().unwrap());
        // No MCP tool is handed a real adapter: the apply ran against the
        // built-in virtual ECU and must say so.
        assert_eq!(apply_res["simulated"], true);
        assert!(apply_res["message"]
            .as_str()
            .unwrap()
            .starts_with("[SIMULATED against the virtual ECU] "));
        assert_eq!(apply_res["steps_completed"].as_u64().unwrap(), 1);
        assert!(apply_res["git_commit_sha"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_mcp_apply_mod_rejects_force_argument() {
        let res = tools::handle_tool_call(
            "sterngate_apply_community_mod",
            &json!({
                "mod_content": "{}",
                "vin": "WDB2112061A123456",
                "battery_voltage": 12.8,
                "force": true
            }),
        )
        .await;
        let err = res.unwrap_err();
        assert!(err.contains("force"), "{err}");
    }

    #[test]
    fn test_mcp_apply_mod_spec_has_no_force_property() {
        let tools = tools::get_tools_list();
        let apply = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "sterngate_apply_community_mod")
            .unwrap();
        assert!(apply["inputSchema"]["properties"].get("force").is_none());
    }

    #[tokio::test]
    async fn test_mcp_apply_mod_requires_vin_and_voltage() {
        let no_vin = tools::handle_tool_call(
            "sterngate_apply_community_mod",
            &json!({ "mod_content": "{}", "battery_voltage": 12.8 }),
        )
        .await
        .unwrap_err();
        assert!(no_vin.contains("vin"), "{no_vin}");

        let no_volts = tools::handle_tool_call(
            "sterngate_apply_community_mod",
            &json!({ "mod_content": "{}", "vin": "WDB2112061A123456" }),
        )
        .await
        .unwrap_err();
        assert!(no_volts.contains("battery voltage"), "{no_volts}");

        let apply = tools::get_tools_list();
        let spec = apply
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "sterngate_apply_community_mod")
            .unwrap();
        let required: Vec<&str> = spec["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(required.contains(&"vin") && required.contains(&"battery_voltage"));
    }

    #[tokio::test]
    async fn test_mcp_tuning_suite() {
        use base64::Engine;

        // Build synthetic 2MB EDC16 ROM
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

        // 1. sterngate_scan_rom_maps
        let scan_res =
            tools::handle_tool_call("sterngate_scan_rom_maps", &json!({ "rom_base64": rom_b64 }))
                .await
                .unwrap();
        assert!(scan_res["success"].as_bool().unwrap());
        assert!(scan_res["map_count"].as_u64().unwrap() > 0);
        assert_eq!(
            scan_res["signatures"]["bosch_hw_id"].as_str().unwrap(),
            "0281012238"
        );

        // 2. sterngate_generate_stage_tune (Stage 1) — refuses: no rom-backed map
        let stage1_err = tools::handle_tool_call(
            "sterngate_generate_stage_tune",
            &json!({ "rom_base64": rom_b64, "stage": 1, "chassis": "W211 E280 CDI", "ecu_name": "EDC16CP31" }),
        )
        .await
        .unwrap_err();
        assert!(stage1_err.contains("Torque Limiter"), "{stage1_err}");

        // 3. sterngate_generate_stage_tune (Stage 2) — refuses for the same reason
        let stage2_err = tools::handle_tool_call(
            "sterngate_generate_stage_tune",
            &json!({ "rom_base64": rom_b64, "stage": 2, "chassis": "W211 E280 CDI", "ecu_name": "EDC16CP31" }),
        )
        .await
        .unwrap_err();
        assert!(stage2_err.contains("provenance"), "{stage2_err}");

        // 4. sterngate_kill_dtc — unsupported until the detector rebuild
        let dtc_err = tools::handle_tool_call(
            "sterngate_kill_dtc",
            &json!({ "rom_base64": rom_b64, "p_codes": ["P0401", "P2002"] }),
        )
        .await
        .unwrap_err();
        assert!(dtc_err.contains("unsupported"), "{dtc_err}");

        // 5. sterngate_solve_checksum (verify)
        let chk_res = tools::handle_tool_call(
            "sterngate_solve_checksum",
            &json!({
                "rom_base64": rom_b64,
                "fix": false
            }),
        )
        .await
        .unwrap();
        assert!(chk_res["success"].as_bool().unwrap());
        assert!(!chk_res["fixed"].as_bool().unwrap());

        // 6. sterngate_solve_checksum (fix)
        let fix_res = tools::handle_tool_call(
            "sterngate_solve_checksum",
            &json!({
                "rom_base64": rom_b64,
                "fix": true
            }),
        )
        .await
        .unwrap();
        assert!(fix_res["success"].as_bool().unwrap());
        assert!(fix_res["fixed"].as_bool().unwrap());
        assert!(fix_res["report"]["is_valid"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_mcp_extended_knowledge_tools() {
        // 1. sterngate_search_workshop_routines
        let r_res = tools::handle_tool_call(
            "sterngate_search_workshop_routines",
            &json!({"query": "steering", "limit": 5}),
        )
        .await
        .unwrap();
        assert!(r_res["count"].as_u64().unwrap() > 0);
        assert!(r_res["total_cataloged"].as_u64().unwrap() >= 1500);

        // 2. sterngate_execute_service_routine
        let exec_res = tools::handle_tool_call(
            "sterngate_execute_service_routine",
            &json!({
                "routine_id": "0x0305",
                "ecu": "CR4",
                "tx_id": 2016,
                "rx_id": 2024
            }),
        )
        .await
        .unwrap();
        assert!(exec_res["success"].as_bool().unwrap());
        assert_eq!(exec_res["routine_id"].as_str().unwrap(), "0x0305");

        // 3. sterngate_search_variant_coding_dids
        let c_res = tools::handle_tool_call(
            "sterngate_search_variant_coding_dids",
            &json!({"query": "vin", "limit": 5}),
        )
        .await
        .unwrap();
        assert!(c_res["count"].as_u64().unwrap() > 0);
        assert!(c_res["total_cataloged"].as_u64().unwrap() >= 3000);

        // 4. sterngate_adapt_donor_ecu_vin
        let revin_res = tools::handle_tool_call(
            "sterngate_adapt_donor_ecu_vin",
            &json!({
                "ecu": "CR4",
                "new_vin": "WDB2112061A888777"
            }),
        )
        .await
        .unwrap();
        assert!(revin_res["success"].as_bool().unwrap());
        assert_eq!(revin_res["new_vin"].as_str().unwrap(), "WDB2112061A888777");
        assert!(revin_res["verified_by_readback"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_mcp_verify_flash_staging_runs_real_preflight() {
        let verify = tools::handle_tool_call("sterngate_verify_flash_staging", &json!({}))
            .await
            .unwrap();
        assert!(verify["passed"].as_bool().unwrap());
        assert!(verify["hw_id_match"].as_bool().unwrap());
        assert!(verify["checksum_match"].as_bool().unwrap());
        assert!(verify["simulated"].as_bool().unwrap());
        assert!(verify["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap().contains("0281012224")));
    }
}
