use serde_json::{json, Value};
use sha2::Digest;
use sterngate_core::{FirmwareVault, FlashPackageManifest};
use sterngate_hal::{VehicleInterface, VirtualCanInterface};
use sterngate_protocol::FlashingWorker;

/// Identifiers the built-in virtual ECU reports for F192/F194, used when a
/// caller does not name one of its own.
const DEMO_HW_ID: &str = "0281012224";
const DEMO_SW_ID: &str = "1037372332";

/// Manifest for the simulated ROM these tools evaluate against the built-in
/// virtual ECU.
///
/// The identifiers are the caller's, so the pre-flight identity gate compares
/// the hardware the caller actually asked about. The checksums are the caller's
/// when supplied and the simulated ROM's own otherwise, so a caller who only
/// wants the identity gate still gets a meaningful checksum verdict instead of
/// an unrelated failure.
fn simulated_manifest(
    target_module: &str,
    expected_hw_id: &str,
    expected_sw_id: &str,
    rom: &[u8],
    sha256_override: Option<&str>,
    crc32_override: Option<u32>,
) -> FlashPackageManifest {
    let sha256_checksum = sha256_override.map(str::to_string).unwrap_or_else(|| {
        let mut hasher = sha2::Sha256::new();
        hasher.update(rom);
        format!("{:x}", hasher.finalize())
    });
    FlashPackageManifest {
        target_module: target_module.to_string(),
        expected_hw_id: expected_hw_id.to_string(),
        expected_sw_id: expected_sw_id.to_string(),
        sha256_checksum,
        crc32_checksum: crc32_override.unwrap_or_else(|| crc32fast::hash(rom)),
        flash_start_address: 0x0004_0000,
        flash_length: u32::try_from(rom.len()).unwrap_or(0),
        block_size: 512,
    }
}

/// The 4096-byte stand-in image both flashing tools evaluate.
fn simulated_rom() -> Vec<u8> {
    vec![0xEA; 4096]
}

pub async fn handle(name: &str, arguments: &Value) -> Result<Value, String> {
    let mut mock_iface = VirtualCanInterface::new();
    let _ = mock_iface.open().await;

    match name {
        "sterngate_verify_flash_staging" => {
            let target_module = arguments
                .get("target_module")
                .and_then(|v| v.as_str())
                .unwrap_or("EDC16");
            // An absent hardware id is not substituted with the one that would
            // pass: pre-flight refuses an empty expected_hw_id on its own.
            let expected_hw_id = arguments
                .get("expected_hw_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let expected_sw_id = arguments
                .get("expected_sw_id")
                .and_then(|v| v.as_str())
                .unwrap_or(DEMO_SW_ID);
            // A checksum the caller did supply is never silently dropped: an
            // unusable value is an error, not a pass.
            let sha256_override = match arguments.get("sha256") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.as_str()),
                Some(_) => return Err("sha256 must be a hex string".into()),
            };
            let crc32_override = match arguments.get("crc32") {
                None | Some(Value::Null) => None,
                Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
                    Some(crc) => Some(crc),
                    None => return Err("crc32 must be a 32-bit unsigned integer".into()),
                },
            };

            let voltage = 13.8;
            let rom = simulated_rom();
            let manifest = simulated_manifest(
                target_module,
                expected_hw_id,
                expected_sw_id,
                &rom,
                sha256_override,
                crc32_override,
            );
            let report = FlashingWorker::new()
                .run_preflight_checks(&manifest, &rom, voltage, &mut mock_iface)
                .await
                .map_err(|e| format!("Pre-flight evaluation failed: {}", e))?;
            let advice = if report.passed {
                "Pre-flight gates PASSED against the built-in virtual ECU. Decoupled worker ready."
            } else {
                "Pre-flight gates FAILED: flashing is refused until every gate passes."
            };
            Ok(json!({
                "passed": report.passed,
                "battery_voltage": voltage,
                "min_voltage_required": report.min_voltage_required,
                "hw_id_match": report.hw_id_match,
                "checksum_match": report.checksum_match,
                "details": report.details,
                "simulated": true,
                "manifest": manifest,
                "advice": advice
            }))
        }
        "sterngate_flash_ecu" => {
            let target_module = arguments
                .get("target_module")
                .and_then(|v| v.as_str())
                .unwrap_or("EDC16");
            let voltage = arguments
                .get("battery_voltage")
                .and_then(|v| v.as_f64())
                .unwrap_or(13.8);
            let dry_run = arguments
                .get("dry_run")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if voltage < 12.5 {
                return Err(format!(
                    "FLASH INTERLOCK VIOLATION: Battery voltage ({:.1}V) is below safe minimum threshold (12.5V). Flashing rejected to prevent ECU bricking.",
                    voltage
                ));
            }

            let dummy_rom = simulated_rom();
            let manifest = simulated_manifest(
                target_module,
                DEMO_HW_ID,
                DEMO_SW_ID,
                &dummy_rom,
                None,
                None,
            );

            if dry_run {
                let report = FlashingWorker::new()
                    .run_preflight_checks(&manifest, &dummy_rom, voltage, &mut mock_iface)
                    .await
                    .map_err(|e| format!("Pre-flight evaluation failed: {}", e))?;
                let status = if report.passed {
                    "Pre-flight safety interlock checks PASSED. Ready for detached flash execution."
                } else {
                    "Pre-flight safety interlock checks FAILED. Flashing is refused; no bytes will be written."
                };
                Ok(json!({
                    "preflight_passed": report.passed,
                    "target_module": target_module,
                    "measured_voltage": voltage,
                    "voltage_threshold_passed": voltage >= report.min_voltage_required,
                    "hw_id_match": report.hw_id_match,
                    "checksum_match": report.checksum_match,
                    "details": report.details,
                    "simulated": true,
                    "manifest": manifest,
                    "status": status
                }))
            } else {
                let flasher = FlashingWorker::new();
                let boxed_iface: Box<dyn VehicleInterface> = Box::new(mock_iface);
                let iface_arc = std::sync::Arc::new(tokio::sync::Mutex::new(boxed_iface));
                match flasher
                    .execute_flash(manifest, dummy_rom, voltage, iface_arc)
                    .await
                {
                    Ok(()) => {
                        let prog = flasher.subscribe().borrow().clone();
                        Ok(json!({
                            "success": true,
                            "target_module": target_module,
                            "measured_voltage": voltage,
                            "state": format!("{:?}", prog.state),
                            "bytes_written": prog.bytes_written,
                            "total_bytes": prog.total_bytes,
                            "percentage": prog.percentage,
                            "log": prog.log,
                            // This executor always drives the in-process virtual
                            // ECU, never a connected vehicle; say so, or the
                            // caller reads a simulation as a real flash.
                            "simulated": true,
                            "message": "Flash sequence completed on the built-in virtual ECU (simulation, no vehicle was written) under an active S3 TesterPresent session."
                        }))
                    }
                    Err(e) => Err(format!("Flashing routine execution failed: {}", e)),
                }
            }
        }
        "sterngate_vault_scan" => {
            let scan_path = arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| FirmwareVault::default_root().display().to_string());
            let entries = FirmwareVault::scan_directory(&scan_path);
            let hw_id = arguments.get("hw_id").and_then(|v| v.as_str());
            let sw_id = arguments.get("sw_id").and_then(|v| v.as_str());
            let recommendation = if let (Some(hw), Some(sw)) = (hw_id, sw_id) {
                FirmwareVault::find_upgrade_recommendation(&entries, hw, sw)
            } else {
                None
            };
            Ok(json!({
                "success": true,
                "scan_path": scan_path,
                "total_files": entries.len(),
                "entries": entries,
                "recommendation": recommendation,
            }))
        }
        _ => Err(format!("Unknown tool name: {}", name)),
    }
}
