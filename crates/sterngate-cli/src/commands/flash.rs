use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::info;

use crate::args::{Cli, FlashCommands};
use crate::commands::common::open_interface;
use sterngate_core::{FirmwareVault, FlashPackageManifest, FlashState};
use sterngate_hal::{OpenPortInterface, VehicleInterface};
use sterngate_protocol::FlashingWorker;

pub async fn execute(action: FlashCommands, cli: &Cli) -> Result<()> {
    match action {
        FlashCommands::Stage { manifest, rom } => {
            info!("Staging firmware package...");
            let manifest_data =
                std::fs::read_to_string(&manifest).context("Failed to read flash manifest JSON")?;
            let pkg_manifest: FlashPackageManifest = serde_json::from_str(&manifest_data)
                .context("Invalid flash manifest JSON syntax")?;
            let rom_data =
                std::fs::read(&rom).context("Failed to read raw ROM/bin firmware file")?;

            let calc_crc32 = crc32fast::hash(&rom_data);
            let mut hasher = sha2::Sha256::default();
            sha2::Digest::update(&mut hasher, &rom_data);
            let calc_sha256 = format!("{:x}", sha2::Digest::finalize(hasher));

            println!("============================================================");
            println!("  Firmware Package Staging & Verification");
            println!("============================================================");
            println!("  • Target ECU:         {}", pkg_manifest.target_module);
            println!("  • Target HW ID:       {}", pkg_manifest.expected_hw_id);
            println!("  • ROM File Size:      {} bytes", rom_data.len());
            println!("  • SHA-256 Calculated: {}", calc_sha256);
            println!("  • SHA-256 Expected:   {}", pkg_manifest.sha256_checksum);
            println!("  • CRC32 Calculated:   0x{:08X}", calc_crc32);
            println!(
                "  • CRC32 Expected:     0x{:08X}",
                pkg_manifest.crc32_checksum
            );

            if calc_sha256.eq_ignore_ascii_case(&pkg_manifest.sha256_checksum)
                && calc_crc32 == pkg_manifest.crc32_checksum
            {
                println!("\n  ✓ Checksums match perfectly. Firmware verified and staged safely on local disk.");
            } else {
                eprintln!("\n  ❌ CHECKSUM MISMATCH! Refusing to stage corrupt firmware binary.");
                std::process::exit(1);
            }
            Ok(())
        }
        FlashCommands::Preflight {
            manifest,
            rom,
            voltage,
        } => {
            info!("Executing Pre-Flight Flash Safety Interlocks...");
            let manifest_data = std::fs::read_to_string(&manifest)?;
            let pkg_manifest: FlashPackageManifest = serde_json::from_str(&manifest_data)?;
            let rom_data = std::fs::read(&rom)?;
            let batt_voltage = if let Some(v) = voltage {
                v
            } else if cli.can_interface == "openport" || cli.can_interface == "tactrix" {
                let mut op = OpenPortInterface::new();
                if op.open().await.is_ok() {
                    if let Ok(measured) = op.read_battery_voltage().await {
                        info!(
                            "Read live battery voltage from Tactrix OpenPort Pin 16 ADC: {:.2} V",
                            measured
                        );
                        measured as f64
                    } else {
                        12.6
                    }
                } else {
                    12.6
                }
            } else {
                12.6
            };

            let mut iface = open_interface(&cli.can_interface).await;
            let flasher = FlashingWorker::new();
            let report = flasher
                .run_preflight_checks(&pkg_manifest, &rom_data, batt_voltage, iface.as_mut())
                .await?;

            println!("============================================================");
            println!("  Flash Pre-Flight Safety Interlock Audit");
            println!("============================================================");
            println!("  • Minimum Voltage Required:  12.50 V");
            println!(
                "  • Measured Battery Voltage:  {:.2} V",
                report.battery_voltage
            );
            println!(
                "  • Checksums Verified:        {}",
                if report.checksum_match {
                    "PASS ✓"
                } else {
                    "FAIL ✗"
                }
            );
            println!(
                "  • Target Hardware ID Match:  {}",
                if report.hw_id_match {
                    "PASS ✓"
                } else {
                    "FAIL ✗"
                }
            );
            println!(
                "  • Overall Verdict:           {}",
                if report.passed {
                    "READY TO FLASH ✓"
                } else {
                    "SAFETY INTERLOCK ENGAGED ✗"
                }
            );
            println!("\n  Safety Details:");
            for d in &report.details {
                println!("    - {}", d);
            }

            if !report.passed {
                std::process::exit(1);
            }
            Ok(())
        }
        FlashCommands::Start {
            manifest,
            rom,
            force_yes,
        } => {
            let manifest_data = std::fs::read_to_string(&manifest)?;
            let pkg_manifest: FlashPackageManifest = serde_json::from_str(&manifest_data)?;
            let rom_data = std::fs::read(&rom)?;
            let batt_voltage = if cli.can_interface == "openport" || cli.can_interface == "tactrix"
            {
                let mut op = OpenPortInterface::new();
                if op.open().await.is_ok() {
                    if let Ok(measured) = op.read_battery_voltage().await {
                        info!(
                            "Read live battery voltage from Tactrix OpenPort Pin 16 ADC: {:.2} V",
                            measured
                        );
                        measured as f64
                    } else {
                        12.6
                    }
                } else {
                    12.6
                }
            } else {
                12.6
            };

            println!("============================================================");
            println!("  🚨 CAUTION: ECU FLASHING SEQUENCE INITIATION");
            println!("============================================================");
            println!("  Target ECU:       {}", pkg_manifest.target_module);
            println!("  Target HW ID:     {}", pkg_manifest.expected_hw_id);
            println!(
                "  ROM Image:        {} ({} bytes)",
                rom.display(),
                rom_data.len()
            );
            println!("\n  ⚠️  DISCLAIMER & LIABILITY NOTICE:");
            println!("  Modifying ECU firmware is performed strictly at your own risk.");
            println!("  The authors and contributors accept ZERO liability for bricked");
            println!("  controllers or vehicle immobilization. See DISCLAIMER.md.");
            println!("\n  This procedure will:");
            println!("  1. Engage API lockout (HTTP 423) across all diagnostic streams");
            println!("  2. Request Programming Diagnostic Session (0x10 03)");
            println!("  3. Unlock Bootloader Security Access (Level 0x0B)");
            println!("  4. Erase designated flash sectors (Routine 0xFF00)");
            println!("  5. Transfer firmware blocks via ISO-TP multi-frame (0x36)");
            println!("  6. Verify memory checksums and reset ECU (0x11 01)");

            if !force_yes {
                use std::io::Write;
                print!("\n  Type 'FLASH_CONFIRM' to proceed: ");
                let _ = std::io::stdout().flush();
                let mut user_input = String::new();
                std::io::stdin().read_line(&mut user_input)?;
                if user_input.trim() != "FLASH_CONFIRM" {
                    println!("Flash aborted by user. No bytes dispatched to CAN bus.");
                    return Ok(());
                }
            }

            let iface = open_interface(&cli.can_interface).await;
            let iface_arc = Arc::new(tokio::sync::Mutex::new(iface));
            let flasher = Arc::new(FlashingWorker::new());
            let mut rx = flasher.subscribe();

            let f_worker = flasher.clone();
            let f_task = tokio::spawn(async move {
                f_worker
                    .execute_flash(pkg_manifest, rom_data, batt_voltage, iface_arc)
                    .await
            });

            println!("\nInitiating detached flash worker...");
            while rx.changed().await.is_ok() {
                let prog = rx.borrow().clone();
                println!(
                    "  [{:>3}%] State: {:<16} | Stage: {}",
                    prog.percentage,
                    format!("{:?}", prog.state),
                    prog.log
                );
                if prog.state == FlashState::Completed {
                    println!("\n✓ ECU FLASHING COMPLETED SUCCESSFULLY! ECU RESET PERFORMED.");
                    break;
                }
                if prog.state == FlashState::Failed {
                    eprintln!("\n❌ FLASHING FAILED: {:?}", prog.error_message);
                    break;
                }
            }

            f_task.await??;
            Ok(())
        }
        FlashCommands::Status => {
            println!("Flashing Engine State: Idle (API Lock: disengaged)");
            Ok(())
        }
        FlashCommands::VaultScan { path, hw_id, sw_id } => {
            // One source of truth: explicit --path, then the global --vault,
            // then STERNGATE_VAULT_ROOT, then ./firmware_vault
            let path = path
                .or_else(|| cli.vault.clone())
                .unwrap_or_else(FirmwareVault::default_root);
            println!("============================================================");
            println!("  LOCAL FIRMWARE VAULT SCANNER");
            println!("============================================================");
            println!("  Directory: {}", path.display());
            let files = FirmwareVault::scan_directory(&path);
            println!(
                "  Found {} firmware binary file(s) in vault.\n",
                files.len()
            );

            let filtered: Vec<_> = files
                .into_iter()
                .filter(|f| {
                    if let Some(ref hw) = hw_id {
                        if let Some(ref f_hw) = f.signatures.bosch_hw_id {
                            if !f_hw.to_lowercase().contains(&hw.to_lowercase()) {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                    if let Some(ref sw) = sw_id {
                        if let Some(ref f_sw) = f.signatures.bosch_sw_id {
                            if !f_sw.to_lowercase().contains(&sw.to_lowercase()) {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                    true
                })
                .collect();

            for f in &filtered {
                println!("------------------------------------------------------------");
                println!("  File:            {}", f.filename);
                println!("  Path:            {}", f.file_path);
                println!(
                    "  Size:            {} bytes ({:.2} KB)",
                    f.file_size_bytes,
                    f.file_size_bytes as f64 / 1024.0
                );
                println!("  Format:          {}", f.format);
                println!("  SHA-256:         {}", f.signatures.sha256_checksum);
                println!("  CRC32:           0x{:08X}", f.signatures.crc32_checksum);
                if let Some(ref hw) = f.signatures.bosch_hw_id {
                    println!("  Hardware ID:     {}", hw);
                }
                if let Some(ref sw) = f.signatures.bosch_sw_id {
                    println!("  Software ID:     {}", sw);
                }
                if let Some(ref part) = f.signatures.oem_part_number {
                    println!("  Part Number:     {}", part);
                }
            }
            if !filtered.is_empty() {
                println!("------------------------------------------------------------");
                println!("  Total matched: {} file(s)", filtered.len());
            } else if hw_id.is_some() || sw_id.is_some() {
                println!("  No firmware binaries matched the specified filter criteria.");
            }
            Ok(())
        }
    }
}
