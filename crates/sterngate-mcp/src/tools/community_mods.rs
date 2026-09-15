use serde_json::{json, Value};
use sterngate_core::{
    encode_to_armor, EcuCatalog, ModAction, ModCategory, ModMetadata, ModRiskLevel,
    ModTargetFilter, SterngateMod,
};
use sterngate_hal::{VehicleInterface, VirtualCanInterface};
use sterngate_protocol::{ModRunner, TargetFingerprintPolicy};

use super::helpers::{parse_hex_slice, parse_mod_content};

pub async fn handle(name: &str, arguments: &Value) -> Result<Value, String> {
    let mut mock_iface = VirtualCanInterface::new();
    let _ = mock_iface.open().await;

    match name {
        "sterngate_inspect_community_mod" => {
            let mod_content = arguments
                .get("mod_content")
                .and_then(|v| v.as_str())
                .ok_or("Missing required 'mod_content'")?;
            let vin = arguments.get("vin").and_then(|v| v.as_str());
            let battery_voltage = arguments.get("battery_voltage").and_then(|v| v.as_f64());

            let mut modpack = parse_mod_content(mod_content)?;
            let report = modpack
                .verify_and_repair()
                .map_err(|e| format!("Verification failed: {}", e))?;

            let compat_report = modpack.check_compatibility(vin, None, battery_voltage);

            Ok(json!({
                "success": true,
                "mod": {
                    "id": modpack.metadata.mod_id,
                    "name": modpack.metadata.name,
                    "version": modpack.metadata.version,
                    "author": modpack.metadata.author,
                    "description": modpack.metadata.description,
                    "category": modpack.metadata.category.as_str(),
                    "risk_level": modpack.metadata.risk_level.as_str(),
                    "instructions": modpack.metadata.instructions,
                    "target": {
                        "chassis": modpack.target.chassis,
                        "ecu_name": modpack.target.ecu_name,
                        "tx_id": format!("0x{:03X}", modpack.target.tx_id),
                        "rx_id": format!("0x{:03X}", modpack.target.rx_id),
                        "min_battery_voltage": modpack.target.min_battery_voltage,
                        "compatible_hw_ids": modpack.target.compatible_hw_ids,
                    },
                    "actions_count": modpack.actions.len(),
                    "actions": modpack.actions,
                },
                "integrity": {
                    "is_valid": report.is_valid,
                    "crc32_verified": report.crc32_verified,
                    "sha256_verified": report.sha256_verified,
                    "fec_status": report.fec_status,
                    "warning_messages": report.warning_messages,
                },
                "compatibility": {
                    "matched_vehicle": compat_report.matched_vehicle,
                    "notes": compat_report.compatibility_notes,
                    "warnings": compat_report.warning_messages,
                }
            }))
        }
        "sterngate_apply_community_mod" => {
            if arguments.get("force").is_some() {
                return Err(
                    "'force' is not accepted over MCP: fingerprint bypass is CLI-only (--force) and never applies to flash writes"
                        .into(),
                );
            }

            let mod_content = arguments
                .get("mod_content")
                .and_then(|v| v.as_str())
                .ok_or("Missing required 'mod_content'")?;
            let vin = arguments
                .get("vin")
                .and_then(|v| v.as_str())
                .unwrap_or("WDB2110001A000000");
            let battery_voltage = arguments
                .get("battery_voltage")
                .and_then(|v| v.as_f64())
                .unwrap_or(13.0);

            let mut modpack = parse_mod_content(mod_content)?;
            let exec_report = ModRunner::apply_mod(
                &mut mock_iface,
                &mut modpack,
                vin,
                battery_voltage,
                TargetFingerprintPolicy::Enforce,
            )
            .await
            .map_err(|e| format!("Mod execution failed: {}", e))?;

            Ok(json!({
                "success": exec_report.success,
                "mod_id": exec_report.mod_id,
                "mod_name": exec_report.mod_name,
                "steps_completed": exec_report.steps_completed,
                "total_steps": exec_report.total_steps,
                "actions_executed": exec_report.actions_executed,
                "git_commit_sha": exec_report.git_commit_sha,
                "message": exec_report.message,
            }))
        }
        "sterngate_create_community_mod" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'name'")?;
            let author = arguments
                .get("author")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'author'")?;
            let description = arguments
                .get("description")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'description'")?;
            let ecu = arguments
                .get("ecu")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'ecu'")?;
            let did_str = arguments
                .get("did")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'did'")?;
            let data_str = arguments
                .get("data")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'data'")?;
            let chassis = arguments
                .get("chassis")
                .and_then(|v| v.as_str())
                .unwrap_or("W211");
            let category_str = arguments
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("comfort");
            let risk_str = arguments
                .get("risk_level")
                .and_then(|v| v.as_str())
                .unwrap_or("low");
            let min_voltage = arguments
                .get("min_voltage")
                .and_then(|v| v.as_f64())
                .unwrap_or(12.0);
            let instructions = arguments.get("instructions").and_then(|v| v.as_str());
            let output_path = arguments.get("output_path").and_then(|v| v.as_str());

            let did = u16::from_str_radix(did_str.trim_start_matches("0x"), 16)
                .map_err(|e| format!("Invalid DID hex: {}", e))?;
            let data = parse_hex_slice(data_str)?;
            let bitmask = match arguments.get("bitmask").and_then(|v| v.as_str()) {
                Some(m) if !m.is_empty() => Some(parse_hex_slice(m)?),
                _ => None,
            };
            let expected = match arguments
                .get("expected_original_data")
                .and_then(|v| v.as_str())
            {
                Some(e) if !e.is_empty() => Some(parse_hex_slice(e)?),
                _ => None,
            };

            let catalog = EcuCatalog::load_default();
            let (tx_id, rx_id) = if let Ok(cat) = catalog {
                if let Some(info) = cat.get_ecu(ecu) {
                    let tx = info
                        .tx_id
                        .as_deref()
                        .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                        .unwrap_or(0x7E0);
                    let rx = info
                        .rx_id
                        .as_deref()
                        .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                        .unwrap_or(0x7E8);
                    (tx, rx)
                } else if ecu.eq_ignore_ascii_case("EGS52") {
                    (0x7E1, 0x7E9)
                } else {
                    (0x7E0, 0x7E8)
                }
            } else if ecu.eq_ignore_ascii_case("EGS52") {
                (0x7E1, 0x7E9)
            } else {
                (0x7E0, 0x7E8)
            };

            let risk_level = match risk_str.to_lowercase().as_str() {
                "moderate" => ModRiskLevel::Moderate,
                "high" | "expert" => ModRiskLevel::High,
                _ => ModRiskLevel::Low,
            };

            let category = match category_str.to_lowercase().as_str() {
                "performance" | "tune" => ModCategory::Performance,
                "transmission" | "trans" => ModCategory::Transmission,
                "lighting" | "light" => ModCategory::Lighting,
                "brakes" | "sbc" => ModCategory::Brakes,
                "emissions" | "egr" => ModCategory::Emissions,
                "retrofit" => ModCategory::Retrofit,
                _ => ModCategory::Comfort,
            };

            let mod_id = format!(
                "{}_{}",
                name.to_lowercase()
                    .replace(' ', "_")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_')
                    .collect::<String>(),
                chrono::Utc::now().format("%Y%m%d")
            );

            let metadata = ModMetadata {
                mod_id: mod_id.clone(),
                name: name.to_string(),
                version: "1.0.0".to_string(),
                author: author.to_string(),
                description: description.to_string(),
                category,
                risk_level,
                instructions: instructions.map(|s| s.to_string()),
                created_at: chrono::Utc::now().to_rfc3339(),
            };

            let target = ModTargetFilter {
                chassis: vec![chassis.to_string()],
                ecu_name: ecu.to_string(),
                tx_id,
                rx_id,
                compatible_hw_ids: vec![],
                compatible_sw_ids: vec![],
                min_battery_voltage: min_voltage,
                requires_engine_off: true,
            };

            let action = ModAction::WriteDid {
                did,
                data,
                bitmask,
                expected_original_data: expected,
                description: format!("Configure DID 0x{:04X} on {}", did, ecu),
            };

            let modpack = SterngateMod::create(metadata, target, vec![action], vec![])
                .map_err(|e| format!("Failed creating modpack: {}", e))?;

            let armored =
                encode_to_armor(&modpack).map_err(|e| format!("Failed encoding armor: {}", e))?;

            if let Some(path_str) = output_path {
                let p = std::path::Path::new(path_str);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(p, modpack.to_json().map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Failed writing file: {}", e))?;
            }

            Ok(json!({
                "success": true,
                "mod_id": mod_id,
                "name": name,
                "parity_bytes": modpack.integrity.fec_parity_bytes.len(),
                "crc32": format!("0x{:08X}", modpack.integrity.payload_crc32),
                "armored_text": armored,
                "output_path": output_path,
            }))
        }
        _ => Err(format!("Unknown tool name: {}", name)),
    }
}
