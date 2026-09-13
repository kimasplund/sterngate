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
    async fn test_mcp_search_cbf_catalog_tool() {
        let res =
            tools::handle_tool_call("sterngate_search_cbf_catalog", &json!({"query": "EGS52"}))
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
}
