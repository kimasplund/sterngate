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
        assert!(stats.get("total_cbf_files").is_some() || stats.get("total_files").is_some());
    }
}
