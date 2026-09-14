use serde_json::{json, Value};
use sterngate_core::{
    encode_to_armor, BoschChecksumSolver, BoschMapDetector, FirmwareSignatures, StageGenerator,
};

use super::helpers::load_rom_bytes_mcp;

pub async fn handle(name: &str, arguments: &Value) -> Result<Value, String> {
    match name {
        "sterngate_scan_rom_maps" => {
            let rom = load_rom_bytes_mcp(arguments)?;
            let sigs = FirmwareSignatures::extract(&rom);
            let checksum = BoschChecksumSolver::verify(&rom);
            let maps = BoschMapDetector::scan_rom(&rom);

            Ok(json!({
                "success": true,
                "rom_size": rom.len(),
                "signatures": sigs,
                "checksum": checksum,
                "map_count": maps.len(),
                "maps": maps,
            }))
        }
        "sterngate_generate_stage_tune" => {
            let rom = load_rom_bytes_mcp(arguments)?;
            let stage = arguments.get("stage").and_then(|v| v.as_u64()).unwrap_or(1);
            let chassis = arguments
                .get("chassis")
                .and_then(|v| v.as_str())
                .unwrap_or("W211");
            let ecu_name = arguments
                .get("ecu_name")
                .and_then(|v| v.as_str())
                .unwrap_or("EDC16CP31");
            let author = arguments
                .get("author")
                .and_then(|v| v.as_str())
                .unwrap_or("Sterngate Tuner");
            let output_path = arguments.get("output_path").and_then(|v| v.as_str());

            let modpack = if stage == 2 {
                StageGenerator::generate_stage2(&rom, chassis, ecu_name, author)
            } else {
                StageGenerator::generate_stage1(&rom, chassis, ecu_name, author)
            }
            .map_err(|e| format!("Failed to generate Stage {} tune: {}", stage, e))?;

            let armored = encode_to_armor(&modpack).unwrap_or_default();

            if let Some(path_str) = output_path {
                let p = std::path::Path::new(path_str);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(p, modpack.to_json().map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Failed writing mod file: {}", e))?;
            }

            Ok(json!({
                "success": true,
                "stage": stage,
                "mod_id": modpack.metadata.mod_id,
                "name": modpack.metadata.name,
                "actions_count": modpack.actions.len(),
                "parity_bytes": modpack.integrity.fec_parity_bytes.len(),
                "crc32": format!("0x{:08X}", modpack.integrity.payload_crc32),
                "armored_text": armored,
                "output_path": output_path,
                "mod": modpack,
            }))
        }
        "sterngate_kill_dtc" => {
            let rom = load_rom_bytes_mcp(arguments)?;
            let p_codes: Vec<String> = arguments
                .get("p_codes")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            if p_codes.is_empty() {
                return Err(
                    "Parameter 'p_codes' must contain at least one DTC code (e.g. ['P0401'])"
                        .to_string(),
                );
            }

            let chassis = arguments
                .get("chassis")
                .and_then(|v| v.as_str())
                .unwrap_or("W211");
            let ecu_name = arguments
                .get("ecu_name")
                .and_then(|v| v.as_str())
                .unwrap_or("EDC16");
            let author = arguments
                .get("author")
                .and_then(|v| v.as_str())
                .unwrap_or("Sterngate Tuner");
            let output_path = arguments.get("output_path").and_then(|v| v.as_str());

            let modpack =
                StageGenerator::generate_dtc_kill(&rom, chassis, ecu_name, &p_codes, author)
                    .map_err(|e| format!("Failed to generate DTC kill mod: {}", e))?;

            let armored = encode_to_armor(&modpack).unwrap_or_default();

            if let Some(path_str) = output_path {
                let p = std::path::Path::new(path_str);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(p, modpack.to_json().map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Failed writing mod file: {}", e))?;
            }

            Ok(json!({
                "success": true,
                "mod_id": modpack.metadata.mod_id,
                "name": modpack.metadata.name,
                "killed_codes": p_codes,
                "actions_count": modpack.actions.len(),
                "parity_bytes": modpack.integrity.fec_parity_bytes.len(),
                "crc32": format!("0x{:08X}", modpack.integrity.payload_crc32),
                "armored_text": armored,
                "output_path": output_path,
                "mod": modpack,
            }))
        }
        "sterngate_solve_checksum" => {
            let mut rom = load_rom_bytes_mcp(arguments)?;
            let fix = arguments
                .get("fix")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let output_path = arguments.get("output_path").and_then(|v| v.as_str());

            if fix {
                let report = BoschChecksumSolver::recalculate_and_apply(&mut rom)
                    .map_err(|e| format!("Failed to recalculate checksums: {}", e))?;

                if let Some(path_str) = output_path {
                    let p = std::path::Path::new(path_str);
                    if let Some(parent) = p.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    std::fs::write(p, &rom)
                        .map_err(|e| format!("Failed writing patched ROM: {}", e))?;
                }

                Ok(json!({
                    "success": true,
                    "fixed": true,
                    "report": report,
                    "output_path": output_path,
                }))
            } else {
                let report = BoschChecksumSolver::verify(&rom);
                Ok(json!({
                    "success": true,
                    "fixed": false,
                    "report": report,
                }))
            }
        }
        _ => Err(format!("Unknown tool name: {}", name)),
    }
}
