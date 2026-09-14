use serde_json::{json, Value};
use sterngate_core::{EcuCatalog, VehicleGarage, VehicleProfile};

pub async fn handle(name: &str, arguments: &Value) -> Result<Value, String> {
    match name {
        "sterngate_list_profiles" => {
            let candidates = ["profiles", "../../profiles", "../profiles"];
            let mut profiles_meta = Vec::new();
            for dir in candidates {
                let p = std::path::Path::new(dir);
                if p.exists() {
                    let discovered = VehicleProfile::discover(p);
                    for prof in discovered {
                        profiles_meta.push(json!({
                            "id": prof.profile_name,
                            "oem": prof.oem,
                            "chassis": prof.chassis,
                            "gateway_type": prof.gateway_type,
                            "default_bitrate": prof.default_bitrate,
                            "modules_count": prof.modules.len(),
                            "parameters_count": prof.parameters.len(),
                            "modules": prof.modules.keys().collect::<Vec<_>>()
                        }));
                    }
                    break;
                }
            }
            if profiles_meta.is_empty() {
                profiles_meta.push(json!({
                    "id": "mercedes_w211_om646_edc16",
                    "oem": "Mercedes-Benz",
                    "chassis": "W211/S211",
                    "modules_count": 2,
                    "parameters_count": 5
                }));
            }
            Ok(json!({
                "count": profiles_meta.len(),
                "profiles": profiles_meta
            }))
        }
        "sterngate_search_ecu_catalog" | "sterngate_search_cbf_catalog" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(25) as usize;

            let catalog = EcuCatalog::load_default()
                .map_err(|e| format!("Failed to load ECU catalog: {}", e))?;
            let results = catalog.search(query, limit);

            Ok(json!({
                "query": query,
                "total_matches": results.len(),
                "results": results
            }))
        }
        "sterngate_inspect_ecu_definition" | "sterngate_inspect_cbf_ecu" => {
            let ecu = arguments
                .get("ecu")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'ecu'".to_string())?;

            let catalog = EcuCatalog::load_default()
                .map_err(|e| format!("Failed to load ECU catalog: {}", e))?;

            if let Some(entry) = catalog.get_ecu(ecu) {
                Ok(serde_json::to_value(entry).unwrap())
            } else {
                Err(format!("ECU '{}' not found in ECU catalog", ecu))
            }
        }
        "sterngate_list_locales" => Ok(json!({
            "locales": [
                { "code": "en", "name": "English", "default": true },
                { "code": "de", "name": "Deutsch (Daimler OEM terminology)", "default": false },
                { "code": "sv", "name": "Svenska", "default": false }
            ]
        })),
        "sterngate_list_vehicles" => {
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let vehicles = garage
                .list_vehicles()
                .map_err(|e| format!("Failed to list vehicles: {}", e))?;
            Ok(json!({
                "total_vehicles": vehicles.len(),
                "vehicles": vehicles
            }))
        }
        "sterngate_import_profiles" => {
            let input_path = arguments
                .get("input_path")
                .and_then(|v| v.as_str())
                .ok_or("Missing required 'input_path'")?;
            let output_dir = arguments
                .get("output_dir")
                .and_then(|v| v.as_str())
                .unwrap_or("profiles");
            match sterngate_protocol::ProfileImporter::import_from_path(input_path, output_dir) {
                Ok(report) => Ok(json!({
                    "success": true,
                    "report": report
                })),
                Err(e) => Err(format!("Profile import failed: {}", e)),
            }
        }
        _ => Err(format!("Unknown tool name: {}", name)),
    }
}
