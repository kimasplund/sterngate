use serde_json::Value;

pub mod analytics;
pub mod community_mods;
pub mod diagnostics;
pub mod flashing;
pub mod helpers;
pub mod service_coding;
pub mod specs;
pub mod tuning;
pub mod vehicle;

pub use specs::get_tools_list;

pub async fn handle_tool_call(name: &str, arguments: &Value) -> Result<Value, String> {
    match name {
        "sterngate_list_interfaces"
        | "sterngate_read_telemetry"
        | "sterngate_read_dtc"
        | "sterngate_clear_dtc"
        | "sterngate_read_parameter"
        | "sterngate_inspect_ecu"
        | "sterngate_control_flight_recorder"
        | "sterngate_scan_vehicle"
        | "sterngate_discover_ecus"
        | "sterngate_export_report" => diagnostics::handle(name, arguments).await,

        "sterngate_verify_flash_staging" | "sterngate_flash_ecu" | "sterngate_vault_scan" => {
            flashing::handle(name, arguments).await
        }

        "sterngate_trigger_routine"
        | "sterngate_service_routine"
        | "sterngate_guided_workflow"
        | "sterngate_search_workshop_routines"
        | "sterngate_execute_service_routine"
        | "sterngate_search_variant_coding_dids"
        | "sterngate_adapt_donor_ecu_vin" => service_coding::handle(name, arguments).await,

        "sterngate_list_profiles"
        | "sterngate_search_ecu_catalog"
        | "sterngate_search_cbf_catalog"
        | "sterngate_inspect_ecu_definition"
        | "sterngate_inspect_cbf_ecu"
        | "sterngate_list_locales"
        | "sterngate_list_vehicles"
        | "sterngate_import_profiles" => vehicle::handle(name, arguments).await,

        "sterngate_analyze_suspension_leak"
        | "sterngate_compare_drive_runs"
        | "sterngate_protect_compressor"
        | "sterngate_control_abc_limiter"
        | "sterngate_check_cascade_warnings" => analytics::handle(name, arguments).await,

        "sterngate_inspect_community_mod"
        | "sterngate_apply_community_mod"
        | "sterngate_create_community_mod" => community_mods::handle(name, arguments).await,

        "sterngate_scan_rom_maps"
        | "sterngate_generate_stage_tune"
        | "sterngate_kill_dtc"
        | "sterngate_solve_checksum" => tuning::handle(name, arguments).await,

        _ => Err(format!("Unknown tool name: {}", name)),
    }
}
