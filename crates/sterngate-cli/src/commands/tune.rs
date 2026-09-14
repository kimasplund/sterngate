use anyhow::{Context, Result};

use crate::args::TuneCommands;
use sterngate_core::{
    encode_to_armor, BoschChecksumSolver, BoschMapDetector, FirmwareSignatures, StageGenerator,
};

pub fn execute(action: TuneCommands) -> Result<()> {
    match action {
        TuneCommands::Scan { rom, filter, table } => {
            let rom_bytes = std::fs::read(&rom)
                .with_context(|| format!("Failed to read ROM file from '{}'", rom.display()))?;
            let sigs = FirmwareSignatures::extract(&rom_bytes);
            let checksum = BoschChecksumSolver::verify(&rom_bytes);
            let maps = BoschMapDetector::scan_rom(&rom_bytes);

            println!("============================================================");
            println!("  ECU ROM CALIBRATION SCAN & MAP DETECTION (WinOLS Engine)");
            println!("============================================================");
            println!(
                "  • File:        {} ({} bytes / {:.2} MB)",
                rom.display(),
                rom_bytes.len(),
                rom_bytes.len() as f64 / (1024.0 * 1024.0)
            );
            println!(
                "  • Bosch HW:    {}",
                sigs.bosch_hw_id.as_deref().unwrap_or("Unknown")
            );
            println!(
                "  • Bosch SW:    {}",
                sigs.bosch_sw_id.as_deref().unwrap_or("Unknown")
            );
            println!(
                "  • Part Number: {}",
                sigs.oem_part_number.as_deref().unwrap_or("Unknown")
            );
            println!("  • Global CRC:  0x{:08X}", checksum.global_crc32);
            println!(
                "  • Checksums:   {} ({} / {} blocks valid)",
                if checksum.is_valid {
                    "PASS ✓"
                } else {
                    "MISMATCH ⚠"
                },
                checksum.valid_blocks,
                checksum.total_blocks
            );

            let filtered_maps: Vec<_> = if let Some(ref f) = filter {
                let f_lower = f.to_lowercase();
                maps.into_iter()
                    .filter(|m| m.name.to_lowercase().contains(&f_lower))
                    .collect()
            } else {
                maps
            };

            println!(
                "\n  [Detected Calibration Maps: {} total]",
                filtered_maps.len()
            );
            for (idx, map) in filtered_maps.iter().enumerate() {
                println!(
                    "  {:2}. [0x{:06X}] {:<36} | {:2}x{:<2} | {}",
                    idx + 1,
                    map.address,
                    map.name,
                    map.rows,
                    map.cols,
                    map.category.as_str()
                );
                if table {
                    println!("\n{}", map.format_ascii_table());
                }
            }
        }
        TuneCommands::Stage1 {
            rom,
            chassis,
            ecu,
            author,
            output,
            armor,
        } => {
            let rom_bytes = std::fs::read(&rom).with_context(|| {
                format!("Failed to read stock ROM file from '{}'", rom.display())
            })?;
            let modpack = StageGenerator::generate_stage1(&rom_bytes, &chassis, &ecu, &author)?;

            println!("============================================================");
            println!("  GENERATED STAGE 1 PERFORMANCE PACKAGE (.sgmod)");
            println!("============================================================");
            println!("  • Name:        {}", modpack.metadata.name);
            println!("  • Mod ID:      {}", modpack.metadata.mod_id);
            println!(
                "  • Target:      {} ({})",
                modpack.target.chassis.join("/"),
                modpack.target.ecu_name
            );
            println!(
                "  • Scope:       +18% Peak Torque (430 Nm max), +120 mbar Boost, +50 bar Rail"
            );
            println!(
                "  • Actions:     {} memory patch routines with rollback protection",
                modpack.actions.len()
            );

            if let Some(ref out_path) = output {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(out_path, modpack.to_json()?)?;
                println!("  ✓ Saved .sgmod to: {}", out_path.display());
            }

            if armor {
                let armored = encode_to_armor(&modpack)?;
                println!("\n--- COPY-PASTEABLE ASCII ARMORED STAGE 1 MOD ---");
                println!("{}", armored);
            }
        }
        TuneCommands::Stage2 {
            rom,
            chassis,
            ecu,
            author,
            output,
            armor,
        } => {
            let rom_bytes = std::fs::read(&rom).with_context(|| {
                format!("Failed to read stock ROM file from '{}'", rom.display())
            })?;
            let modpack = StageGenerator::generate_stage2(&rom_bytes, &chassis, &ecu, &author)?;

            println!("============================================================");
            println!("  GENERATED STAGE 2 RACE PERFORMANCE PACKAGE (.sgmod)");
            println!("============================================================");
            println!("  • Name:        {}", modpack.metadata.name);
            println!("  • Mod ID:      {}", modpack.metadata.mod_id);
            println!(
                "  • Target:      {} ({})",
                modpack.target.chassis.join("/"),
                modpack.target.ecu_name
            );
            println!("  • Scope:       +25% Peak Torque, +200 mbar Boost, +80 bar Rail, DPF Off, EGR Hysteresis Off, P0401/P2002 DTC Suppressed");
            println!("  • Warning:     Requires physical DPF downpipe & EGR blanking plate (Off-road only)");

            if let Some(ref out_path) = output {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(out_path, modpack.to_json()?)?;
                println!("  ✓ Saved .sgmod to: {}", out_path.display());
            }

            if armor {
                let armored = encode_to_armor(&modpack)?;
                println!("\n--- COPY-PASTEABLE ASCII ARMORED STAGE 2 MOD ---");
                println!("{}", armored);
            }
        }
        TuneCommands::DtcKill {
            rom,
            codes,
            chassis,
            ecu,
            author,
            output,
            armor,
        } => {
            let rom_bytes = std::fs::read(&rom).with_context(|| {
                format!("Failed to read stock ROM file from '{}'", rom.display())
            })?;
            let p_codes: Vec<String> = codes
                .split(',')
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty())
                .collect();

            let modpack =
                StageGenerator::generate_dtc_kill(&rom_bytes, &chassis, &ecu, &p_codes, &author)?;

            println!("============================================================");
            println!("  GENERATED STANDALONE DTC SUPPRESSION PACKAGE (.sgmod)");
            println!("============================================================");
            println!("  • Suppressed:  {}", p_codes.join(", "));
            println!("  • Target ECU:  {} ({})", chassis, ecu);

            if let Some(ref out_path) = output {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(out_path, modpack.to_json()?)?;
                println!("  ✓ Saved .sgmod to: {}", out_path.display());
            }

            if armor {
                let armored = encode_to_armor(&modpack)?;
                println!("\n--- COPY-PASTEABLE ASCII ARMORED DTC KILL MOD ---");
                println!("{}", armored);
            }
        }
        TuneCommands::Checksum { rom, fix, output } => {
            let mut rom_bytes = std::fs::read(&rom)
                .with_context(|| format!("Failed to read ROM file from '{}'", rom.display()))?;
            let report = BoschChecksumSolver::verify(&rom_bytes);

            println!("============================================================");
            println!("  BOSCH MPC5xx / FLASH CHECKSUM VERIFICATION");
            println!("============================================================");
            println!("  • File:        {}", rom.display());
            println!("  • Global CRC:  0x{:08X}", report.global_crc32);
            println!(
                "  • Blocks:      {} total ({} valid)",
                report.total_blocks, report.valid_blocks
            );
            println!(
                "  • Status:      {}",
                if report.is_valid {
                    "PASS ✓ ALL CHECKSUMS VALID"
                } else {
                    "MISMATCH ⚠ CHECKSUMS INVALID"
                }
            );

            println!("\n  [Block Map]");
            for b in &report.blocks {
                println!(
                    "  Block #{}: 0x{:06X}..0x{:06X} | Sum: 0x{:08X} (stored: 0x{:08X}) | Inv: 0x{:08X} | {}",
                    b.block_index,
                    b.start_address,
                    b.end_address,
                    b.calculated_sum,
                    b.stored_sum,
                    b.calculated_inv,
                    if b.is_valid { "VALID ✓" } else { "INVALID ✗" }
                );
            }

            if fix {
                let fixed_report = BoschChecksumSolver::recalculate_and_apply(&mut rom_bytes)?;
                let out_path = output.unwrap_or(rom);
                std::fs::write(&out_path, &rom_bytes).with_context(|| {
                    format!("Failed writing fixed ROM to '{}'", out_path.display())
                })?;
                println!(
                    "\n  ✓ Successfully recalculated and fixed all {} checksum blocks!",
                    fixed_report.valid_blocks
                );
                println!("  ✓ Output written to: {}", out_path.display());
            }
        }
    }
    Ok(())
}
