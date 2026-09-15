use anyhow::Result;

use crate::args::{Cli, ModCommands};
use crate::commands::common::{load_mod_input, open_interface, parse_hex_bytes};
use sterngate_core::{
    encode_to_armor, EcuCatalog, FecStatus, ModAction, ModCategory, ModMetadata, ModRiskLevel,
    ModTargetFilter, SterngateMod, VehicleGarage,
};
use sterngate_protocol::ModRunner;

pub async fn execute(action: ModCommands, cli: &Cli) -> Result<()> {
    match action {
        ModCommands::Inspect { input, vin } => {
            let mut modpack = load_mod_input(&input)?;
            println!("============================================================");
            println!("  COMMUNITY MOD INSPECTION & INTEGRITY CHECK");
            println!("============================================================");
            println!("  • Name:        {}", modpack.metadata.name);
            println!("  • ID:          {}", modpack.metadata.mod_id);
            println!("  • Version:     {}", modpack.metadata.version);
            println!("  • Author:      {}", modpack.metadata.author);
            println!("  • Category:    {}", modpack.metadata.category.as_str());
            println!("  • Risk Level:  {}", modpack.metadata.risk_level.as_str());
            println!("  • Description: {}", modpack.metadata.description);
            if let Some(ref inst) = modpack.metadata.instructions {
                println!("  • Instructions: {}", inst);
            }
            println!("\n  [Targeting Filter]");
            println!("  • Chassis:     {}", modpack.target.chassis.join(", "));
            println!("  • Module:      {}", modpack.target.ecu_name);
            println!(
                "  • Tx/Rx CAN:   0x{:03X} / 0x{:03X}",
                modpack.target.tx_id, modpack.target.rx_id
            );
            println!(
                "  • Min Voltage: {:.1} V",
                modpack.target.min_battery_voltage
            );
            if !modpack.target.compatible_hw_ids.is_empty() {
                println!(
                    "  • Whitelisted HW IDs: {}",
                    modpack.target.compatible_hw_ids.join(", ")
                );
            }

            println!("\n  [Actions ({} total)]", modpack.actions.len());
            for (idx, act) in modpack.actions.iter().enumerate() {
                match act {
                    ModAction::WriteDid {
                        did,
                        data,
                        bitmask,
                        expected_original_data,
                        description,
                    } => {
                        println!(
                            "    {}. Write DID 0x{:04X}: {} (bytes: {:02X?})",
                            idx + 1,
                            did,
                            description,
                            data
                        );
                        if let Some(mask) = bitmask {
                            println!("       - Bitmask: {:02X?}", mask);
                        }
                        if let Some(exp) = expected_original_data {
                            println!("       - Precondition required: {:02X?}", exp);
                        }
                    }
                    ModAction::Routine {
                        routine_id,
                        subfunction,
                        data,
                        description,
                    } => {
                        println!(
                            "    {}. Routine 0x{:04X} (subfn 0x{:02X}, data {:02X?}): {}",
                            idx + 1,
                            routine_id,
                            subfunction,
                            data,
                            description
                        );
                    }
                    ModAction::PatchFlashMap {
                        map_name,
                        address_offset,
                        data,
                        expected_original_data,
                        description,
                        ..
                    } => {
                        println!(
                            "    {}. Flash Map Patch '{}' @ 0x{:06X}: {} ({} bytes)",
                            idx + 1,
                            map_name,
                            address_offset,
                            description,
                            data.len()
                        );
                        if let Some(exp) = expected_original_data {
                            println!(
                                "       - Original validation required ({} bytes)",
                                exp.len()
                            );
                        }
                    }
                    ModAction::DtcMask {
                        p_code,
                        address_offset,
                        original_mask,
                        disable_mask,
                        description,
                        ..
                    } => {
                        println!(
                            "    {}. DTC Mask '{}' @ 0x{:06X}: {} (0x{:02X} -> 0x{:02X})",
                            idx + 1,
                            p_code,
                            address_offset,
                            description,
                            original_mask,
                            disable_mask
                        );
                    }
                }
            }

            println!("\n  [Integrity & Forward Error Correction]");
            let report = modpack.verify_and_repair()?;
            if report.is_valid {
                println!(
                    "  • Checksum (CRC32):  ✓ Valid (0x{:08X})",
                    modpack.integrity.payload_crc32
                );
                println!(
                    "  • Checksum (SHA256): ✓ Valid ({})",
                    &modpack.integrity.payload_sha256[..16]
                );
                match &report.fec_status {
                    FecStatus::Repaired {
                        corrected_byte_count,
                        repaired_offsets,
                    } => {
                        println!(
                            "  • Reed-Solomon FEC:  ✓ AUTO-REPAIRED {} corrupted byte(s) at positions: {:?}",
                            corrected_byte_count, repaired_offsets
                        );
                    }
                    _ => {
                        println!("  • Reed-Solomon FEC:  ✓ Parity blocks clean (zero bitflips)");
                    }
                }
            } else {
                println!("  • Checksum (CRC32):  ✗ Corrupted or unrecoverable");
                for warn in &report.warning_messages {
                    println!("    ! {}", warn);
                }
            }

            if let Some(target_vin) = vin {
                println!("\n  [Chassis Compatibility for VIN: {}]", target_vin);
                let compat = modpack.check_compatibility(Some(&target_vin), None, None);
                if compat.matched_vehicle {
                    println!("  ✓ Compatible with chassis {}", target_vin);
                    for note in &compat.compatibility_notes {
                        println!("    {}", note);
                    }
                } else {
                    println!("  ✗ INCOMPATIBLE with chassis {}", target_vin);
                    for warn in &compat.warning_messages {
                        println!("    ! {}", warn);
                    }
                }
            }
            Ok(())
        }
        ModCommands::Apply { input, vin, force } => {
            let mut modpack = load_mod_input(&input)?;
            let target_vin = vin.unwrap_or_else(|| {
                let garage = VehicleGarage::new(VehicleGarage::default_path());
                if let Ok(vehicles) = garage.list_vehicles() {
                    if let Some(v) = vehicles.first() {
                        return v.vin.clone();
                    }
                }
                "WDB2110001A000000".to_string()
            });

            let mut iface = open_interface(&cli.can_interface).await;
            println!("============================================================");
            println!("  APPLYING COMMUNITY MOD: {}", modpack.metadata.name);
            println!("============================================================");
            println!("  Target VIN:     {}", target_vin);
            println!("  Interface:      {}", cli.can_interface);
            println!("  Risk Level:     {}", modpack.metadata.risk_level.as_str());
            if force {
                println!("  ⚠️  FORCED BYPASS OF PRECONDITIONS ENABLED");
            }

            let battery_voltage = 13.2;

            let report = ModRunner::apply_mod(
                iface.as_mut(),
                &mut modpack,
                &target_vin,
                battery_voltage,
                force,
            )
            .await?;

            if report.success {
                println!("\n  ✓ SUCCESS: {}", report.message);
                println!(
                    "  • Steps executed: {}/{}",
                    report.steps_completed, report.total_steps
                );
                for act in &report.actions_executed {
                    println!("    - {}", act);
                }
                if let Some(commit) = &report.git_commit_sha {
                    println!("  • Vehicle Garage Git snapshot: {}", commit);
                }
            } else {
                println!("\n  ✗ FAILED: {}", report.message);
            }
            Ok(())
        }
        ModCommands::Create {
            name,
            author,
            description,
            chassis,
            ecu,
            did,
            data,
            mask,
            expected,
            category,
            risk,
            min_voltage,
            out,
            armor,
        } => {
            let did_u16 = u16::from_str_radix(did.trim_start_matches("0x"), 16)
                .map_err(|e| anyhow::anyhow!("Invalid DID hex '{}': {}", did, e))?;

            let data_bytes = parse_hex_bytes(&data)?;
            let mask_bytes = match mask {
                Some(ref m) => Some(parse_hex_bytes(m)?),
                None => None,
            };
            let exp_bytes = match expected {
                Some(ref e) => Some(parse_hex_bytes(e)?),
                None => None,
            };

            let catalog = EcuCatalog::load_default();
            let (tx_id, rx_id) = if let Ok(cat) = catalog {
                if let Some(info) = cat.get_ecu(&ecu) {
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

            let risk_level = match risk.to_lowercase().as_str() {
                "moderate" => ModRiskLevel::Moderate,
                "high" | "expert" => ModRiskLevel::High,
                _ => ModRiskLevel::Low,
            };

            let mod_category = match category.to_lowercase().as_str() {
                "performance" | "tune" => ModCategory::Performance,
                "transmission" | "trans" | "egs" => ModCategory::Transmission,
                "comfort" => ModCategory::Comfort,
                "lighting" | "light" => ModCategory::Lighting,
                "brakes" | "sbc" | "esp" => ModCategory::Brakes,
                "emissions" | "egr" | "dpf" => ModCategory::Emissions,
                _ => ModCategory::Retrofit,
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
                name: name.clone(),
                version: "1.0.0".into(),
                author: author.clone(),
                description: description.clone(),
                category: mod_category,
                risk_level,
                instructions: Some(
                    "Apply with ignition ON (engine OFF). Maintain battery voltage above minimum."
                        .into(),
                ),
                created_at: chrono::Utc::now().to_rfc3339(),
            };

            let target = ModTargetFilter {
                chassis: vec![chassis.clone()],
                ecu_name: ecu.clone(),
                tx_id,
                rx_id,
                compatible_hw_ids: vec![],
                compatible_sw_ids: vec![],
                min_battery_voltage: min_voltage,
                requires_engine_off: true,
            };

            let action = ModAction::WriteDid {
                did: did_u16,
                data: data_bytes,
                bitmask: mask_bytes,
                expected_original_data: exp_bytes,
                description: format!("Configure DID 0x{:04X} on {}", did_u16, ecu),
            };

            let modpack = SterngateMod::create(metadata, target, vec![action], vec![])?;

            let armored = encode_to_armor(&modpack)?;

            if let Some(out_path) = out {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(&out_path, modpack.to_json()?)?;
                println!("✓ Mod file saved to: {}", out_path.display());
            }

            if armor {
                println!("\n--- COPY-PASTEABLE ASCII ARMORED MOD ---");
                println!("{}", armored);
            }

            println!("✓ Mod '{}' (ID: {}) created successfully.", name, mod_id);
            Ok(())
        }
        ModCommands::List { dir } => {
            println!("============================================================");
            println!("  COMMUNITY MOD LIBRARY: {}", dir.display());
            println!("============================================================");

            if !dir.exists() {
                std::fs::create_dir_all(&dir).ok();
            }

            let mut entries_count = 0;
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path
                        .extension()
                        .is_some_and(|ext| ext == "sgmod" || ext == "json" || ext == "txt")
                    {
                        if let Ok(mut m) = load_mod_input(&path.to_string_lossy()) {
                            entries_count += 1;
                            let rep = m.verify_and_repair();
                            let fec_str = match rep {
                                Ok(r) if r.is_valid => match r.fec_status {
                                    FecStatus::Repaired {
                                        corrected_byte_count,
                                        ..
                                    } => {
                                        format!("✓ Repaired (+{}B)", corrected_byte_count)
                                    }
                                    _ => "✓ Clean".to_string(),
                                },
                                _ => "✗ Corrupt".to_string(),
                            };
                            println!(
                                "• {:<20} | {:<16} | {:<6} | {:<8} | {:<10} | {}",
                                m.metadata.name,
                                m.target.ecu_name,
                                m.target.chassis.join("/"),
                                m.metadata.risk_level.as_str(),
                                fec_str,
                                path.file_name().unwrap_or_default().to_string_lossy()
                            );
                        }
                    }
                }
            }

            if entries_count == 0 {
                println!("  No .sgmod packages found in {}.", dir.display());
                println!(
                    "  Use 'sterngate mod create' or drag-and-drop in the Web UI to add mods."
                );
            } else {
                println!("\nTotal: {} community mod(s) found.", entries_count);
            }
            Ok(())
        }
    }
}
