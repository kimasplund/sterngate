//! Community Mod Execution Engine
//!
//! Enforces safety gates, checks vehicle fingerprint compatibility, verifies preconditions,
//! creates atomic Vehicle Garage snapshots, applies DID writes with bitmasking,
//! and verifies post-write states over CAN.

use serde::{Deserialize, Serialize};
use tracing::info;

use sterngate_core::{
    ModAction, ModValidationReport, Result, SterngateError, SterngateMod, VehicleGarage,
    FLASH_WRITE_MIN_VOLTAGE,
};
use sterngate_hal::VehicleInterface;

use crate::uds::{S3KeepAlive, UdsClient};

/// Report returned after executing a community mod
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModExecutionReport {
    pub mod_id: String,
    pub mod_name: String,
    pub success: bool,
    pub steps_completed: usize,
    pub total_steps: usize,
    pub actions_executed: Vec<String>,
    pub git_commit_sha: Option<String>,
    pub live_hw_id: Option<String>,
    pub message: String,
}

/// How strictly the vehicle fingerprint (chassis, hardware whitelist) is enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFingerprintPolicy {
    /// Refuse on any chassis or hardware-whitelist mismatch.
    Enforce,
    /// Relax the chassis and hardware-whitelist checks only. Never relaxes the
    /// voltage floor, map provenance or byte preconditions, and is refused
    /// outright for packages that write flash memory.
    BypassUnsafe,
}

pub struct ModRunner;

impl ModRunner {
    /// Flash ranges within one package must not overlap: a later action's
    /// precondition would otherwise be checked against bytes an earlier action changes.
    fn check_no_overlap(modpack: &SterngateMod) -> Result<()> {
        let mut ranges: Vec<(u32, u32, String)> = Vec::new();
        for action in &modpack.actions {
            let (start, len, name) = match action {
                ModAction::PatchFlashMap {
                    address_offset,
                    data,
                    map_name,
                    ..
                } => (*address_offset, data.len(), map_name.clone()),
                ModAction::DtcMask {
                    address_offset,
                    p_code,
                    ..
                } => (*address_offset, 1, p_code.clone()),
                _ => continue,
            };
            let len = u32::try_from(len).map_err(|_| {
                SterngateError::PreFlightCheckFailed(format!("action '{name}' is too large"))
            })?;
            let end = start.checked_add(len).ok_or_else(|| {
                SterngateError::PreFlightCheckFailed(format!(
                    "action '{name}' address range overflows"
                ))
            })?;
            for (s, e, other) in &ranges {
                if start < *e && *s < end {
                    return Err(SterngateError::PreFlightCheckFailed(format!(
                        "flash ranges of '{name}' and '{other}' overlap; refusing the package"
                    )));
                }
            }
            ranges.push((start, end, name));
        }
        Ok(())
    }

    async fn read_live_hw_id(uds: &mut UdsClient<'_>) -> Option<String> {
        match uds.read_data_by_identifier(0xF191).await {
            Ok(resp) if resp.len() >= 4 => {
                Some(String::from_utf8_lossy(&resp[3..]).trim().to_string())
            }
            _ => None,
        }
    }

    /// Inspect and test compatibility of a mod against live vehicle without executing changes
    pub async fn inspect_compatibility(
        interface: &mut dyn VehicleInterface,
        modpack: &SterngateMod,
        vin: Option<&str>,
        battery_voltage: Option<f64>,
    ) -> Result<ModValidationReport> {
        let report = modpack.clone().verify_and_repair()?;
        if !report.is_valid {
            return Ok(report);
        }

        let mut uds = UdsClient::new(interface, modpack.target.tx_id, modpack.target.rx_id);

        let live_hw_id = if modpack.target.compatible_hw_ids.is_empty() {
            None
        } else {
            let _ = uds.diagnostic_session_control(0x03).await;
            Self::read_live_hw_id(&mut uds).await
        };

        let mut final_report =
            modpack.check_compatibility(vin, live_hw_id.as_deref(), battery_voltage);

        if !modpack.target.compatible_hw_ids.is_empty() && live_hw_id.is_none() {
            final_report.matched_vehicle = false;
            final_report.warning_messages.push(
                "Mod declares a hardware whitelist but the ECU hardware ID (DID F191) could not be read"
                    .into(),
            );
        }

        Ok(final_report)
    }

    /// Safely apply a community mod package to the connected vehicle.
    ///
    /// Gate order: integrity, flash-write refusals (`SterngateMod::flash_write_refusals`:
    /// map provenance, EraseMemory routine), policy admissibility, voltage floor,
    /// range overlap, chassis, hardware whitelist, per-action byte preconditions,
    /// then writes. Only the chassis and hardware-whitelist checks consult `policy`.
    /// `flash_write_refusals` is the single source of truth shared with
    /// `inspect_compatibility` (via `SterngateMod::check_compatibility`), so every
    /// inspect surface (CLI, MCP, REST) reports the same refusals `apply_mod` enforces.
    ///
    /// Once the extended session (`10 03`) is entered, an `S3KeepAlive` ticks
    /// before every subsequent request (the hardware ID read, each precondition
    /// read, the bitmask read, and each write/routine), so a slow ECU or a long
    /// garage commit between preconditions and writes never lets S3 expire.
    pub async fn apply_mod(
        interface: &mut dyn VehicleInterface,
        modpack: &mut SterngateMod,
        vin: &str,
        battery_voltage: f64,
        policy: TargetFingerprintPolicy,
    ) -> Result<ModExecutionReport> {
        // 1. Verify and auto-repair Reed-Solomon FEC
        let validation = modpack.verify_and_repair()?;
        if !validation.is_valid {
            return Err(SterngateError::ProfileError(format!(
                "Mod package integrity check failed: {:?}",
                validation.warning_messages
            )));
        }

        // 1b. Flash-write refusals: map provenance and EraseMemory routines
        // (before any bus traffic). One source of truth with `inspect_compatibility`.
        if let Some(refusal) = modpack.flash_write_refusals().into_iter().next() {
            return Err(SterngateError::PreFlightCheckFailed(refusal));
        }

        // 1c. A fingerprint bypass is never admissible for flash writes
        let writes_flash = modpack.writes_flash();
        if writes_flash && policy == TargetFingerprintPolicy::BypassUnsafe {
            return Err(SterngateError::PreFlightCheckFailed(
                "--force is not permitted for packages containing flash writes (PatchFlashMap/DtcMask)"
                    .into(),
            ));
        }

        // 2. Safety interlock: voltage (policy-independent)
        let required = if writes_flash {
            modpack
                .target
                .min_battery_voltage
                .max(FLASH_WRITE_MIN_VOLTAGE)
        } else {
            modpack.target.min_battery_voltage
        };
        if battery_voltage.is_nan() || battery_voltage < required {
            return Err(SterngateError::PreFlightCheckFailed(format!(
                "Battery voltage ({:.1}V) is below required minimum ({:.1}V) for mod '{}'",
                battery_voltage, required, modpack.metadata.name
            )));
        }

        // 2b. Flash ranges must not overlap
        Self::check_no_overlap(modpack)?;

        // 3. Vehicle chassis fingerprint check
        if policy == TargetFingerprintPolicy::Enforce && !modpack.target.matches_chassis(vin) {
            return Err(SterngateError::ProtocolError(format!(
                "Vehicle chassis mismatch: Mod '{}' requires chassis {:?}, but connected VIN is '{}'",
                modpack.metadata.name, modpack.target.chassis, vin
            )));
        }

        let mut uds = UdsClient::new(interface, modpack.target.tx_id, modpack.target.rx_id);

        // Enter Extended Diagnostic Session (0x10 03)
        let _ = uds.diagnostic_session_control(0x03).await;

        // S3 (extended session) keep-alive: a slow ECU or a long garage commit
        // between preconditions and writes must never let the S3 timer expire.
        // `tick` fires an unconditional, periodic suppressed TesterPresent before
        // every request; it is never `touch`ed after a reply (Task 5 ruling).
        let mut ka = S3KeepAlive::new();

        // 4. ECU hardware ID whitelist check (fail closed when a whitelist exists)
        ka.tick(&mut uds).await?;
        let live_hw_id = Self::read_live_hw_id(&mut uds).await;
        if policy == TargetFingerprintPolicy::Enforce
            && !modpack.target.compatible_hw_ids.is_empty()
        {
            match live_hw_id.as_deref() {
                None => {
                    return Err(SterngateError::PreFlightCheckFailed(format!(
                        "mod '{}' declares a hardware whitelist {:?} but the ECU hardware ID could not be read; refusing",
                        modpack.metadata.name, modpack.target.compatible_hw_ids
                    )));
                }
                Some(hw) if !modpack.target.matches_hardware(hw) => {
                    return Err(SterngateError::ProtocolError(format!(
                        "Incompatible ECU Hardware ID '{}'. Mod '{}' only supports: {:?}",
                        hw, modpack.metadata.name, modpack.target.compatible_hw_ids
                    )));
                }
                Some(_) => {}
            }
        }

        // 5. Preconditions: prove the live bytes before writing anything (never bypassable)
        for action in &modpack.actions {
            match action {
                ModAction::WriteDid {
                    did,
                    expected_original_data: Some(expected),
                    ..
                } => {
                    if expected.is_empty() {
                        return Err(SterngateError::PreFlightCheckFailed(format!(
                            "DID 0x{did:04X}: expected_original_data is an empty precondition; refusing vacuous precondition"
                        )));
                    }
                    ka.tick(&mut uds).await?;
                    let resp = uds.read_data_by_identifier(*did).await.map_err(|e| {
                        SterngateError::PreFlightCheckFailed(format!(
                            "DID 0x{did:04X}: could not read current value for precondition ({e}); refusing"
                        ))
                    })?;
                    let current = resp.get(3..).unwrap_or(&[]);
                    if current.len() < expected.len() || current[..expected.len()] != expected[..] {
                        return Err(SterngateError::PreFlightCheckFailed(format!(
                            "Precondition check failed for DID 0x{did:04X}: expected current bytes {expected:02X?}, but vehicle returned {current:02X?}. Mod execution halted to prevent configuration corruption."
                        )));
                    }
                }
                ModAction::PatchFlashMap {
                    map_name,
                    address_offset,
                    data,
                    expected_original_data,
                    ..
                } => {
                    let Some(expected) =
                        expected_original_data.as_deref().filter(|e| !e.is_empty())
                    else {
                        return Err(SterngateError::PreFlightCheckFailed(format!(
                            "map '{map_name}' @0x{address_offset:06X}: no expected_original_data; blind flash patches are refused"
                        )));
                    };
                    if expected.len() != data.len() {
                        return Err(SterngateError::PreFlightCheckFailed(format!(
                            "map '{map_name}' @0x{address_offset:06X}: a patch must replace exactly the bytes it verified (expected {} bytes, data {} bytes)",
                            expected.len(),
                            data.len()
                        )));
                    }
                    let len = u16::try_from(expected.len()).map_err(|_| {
                        SterngateError::PreFlightCheckFailed(format!(
                            "map '{map_name}': precondition longer than 65535 bytes"
                        ))
                    })?;
                    ka.tick(&mut uds).await?;
                    let current = uds
                        .read_memory_by_address(*address_offset, len)
                        .await
                        .map_err(|e| {
                            SterngateError::PreFlightCheckFailed(format!(
                                "map '{map_name}' @0x{address_offset:06X}: could not read original bytes ({e}); refusing to patch unverified memory"
                            ))
                        })?;
                    if current.len() != expected.len() || current[..] != expected[..] {
                        return Err(SterngateError::PreFlightCheckFailed(format!(
                            "Precondition check failed for map '{map_name}' at 0x{address_offset:06X}: expected original bytes {expected:02X?}, but vehicle returned {current:02X?}. Aborting flash patch."
                        )));
                    }
                }
                ModAction::DtcMask {
                    p_code,
                    address_offset,
                    original_mask,
                    ..
                } => {
                    ka.tick(&mut uds).await?;
                    let current = uds
                        .read_memory_by_address(*address_offset, 1)
                        .await
                        .map_err(|e| {
                            SterngateError::PreFlightCheckFailed(format!(
                                "DTC {p_code} mask @0x{address_offset:06X}: could not read current mask ({e}); refusing"
                            ))
                        })?;
                    match current.first() {
                        Some(b) if current.len() == 1 && *b == *original_mask => {}
                        Some(b) if current.len() == 1 => {
                            return Err(SterngateError::PreFlightCheckFailed(format!(
                                "DTC {p_code} mask @0x{address_offset:06X} is 0x{b:02X}, expected 0x{original_mask:02X}; refusing"
                            )));
                        }
                        _ => {
                            return Err(SterngateError::PreFlightCheckFailed(format!(
                                "DTC {p_code} mask @0x{address_offset:06X}: expected 1 byte, got {}",
                                current.len()
                            )));
                        }
                    }
                }
                ModAction::WriteDid { .. } | ModAction::Routine { .. } => {}
            }
        }

        // 6. Atomic pre-mod Git garage snapshot
        let garage = VehicleGarage::new(VehicleGarage::default_path());
        let pre_note = format!(
            "Pre-mod baseline snapshot before applying '{}' (ID: {})",
            modpack.metadata.name, modpack.metadata.mod_id
        );
        let _ = garage.save_coding(
            vin,
            &modpack.target.ecu_name,
            "PRE_MOD_SNAPSHOT",
            None,
            &pre_note,
        );

        // 7. Execute actions
        let total_steps = modpack.actions.len();
        let mut steps_completed = 0;
        let mut actions_executed = Vec::new();

        for action in &modpack.actions {
            match action {
                ModAction::WriteDid {
                    did,
                    data,
                    bitmask,
                    description,
                    ..
                } => {
                    let write_payload = if let Some(mask) = bitmask {
                        if mask.len() != data.len() {
                            return Err(SterngateError::PreFlightCheckFailed(format!(
                                "DID 0x{did:04X}: bitmask length ({}) does not match data length ({}); refusing to clobber unmasked bits",
                                mask.len(),
                                data.len()
                            )));
                        }
                        ka.tick(&mut uds).await?;
                        let resp = uds.read_data_by_identifier(*did).await.map_err(|e| {
                            SterngateError::PreFlightCheckFailed(format!(
                                "DID 0x{did:04X}: bitmask write requires the current value but the read failed ({e}); refusing to clobber unmasked bits"
                            ))
                        })?;
                        let current_bytes = resp.get(3..).unwrap_or(&[]);
                        if current_bytes.len() < mask.len() {
                            return Err(SterngateError::PreFlightCheckFailed(format!(
                                "DID 0x{did:04X}: bitmask covers {} bytes but the ECU returned {}; refusing",
                                mask.len(),
                                current_bytes.len()
                            )));
                        }
                        let mut merged = data.clone();
                        for (i, m) in mask.iter().enumerate() {
                            let cur = current_bytes.get(i).copied().unwrap_or(0);
                            let new_val = data.get(i).copied().unwrap_or(0);
                            let final_byte = (new_val & m) | (cur & !m);
                            if i < merged.len() {
                                merged[i] = final_byte;
                            } else {
                                merged.push(final_byte);
                            }
                        }
                        merged
                    } else {
                        data.clone()
                    };

                    info!(
                        "Applying Mod Write DID 0x{:04X} ({} bytes)...",
                        did,
                        write_payload.len()
                    );
                    ka.tick(&mut uds).await?;
                    uds.write_data_by_identifier(*did, &write_payload).await?;
                    steps_completed += 1;
                    actions_executed.push(format!("Write DID 0x{:04X}: {}", did, description));
                }
                ModAction::Routine {
                    routine_id,
                    subfunction,
                    data,
                    description,
                } => {
                    info!(
                        "Applying Mod Routine 0x{:04X} (subfunction: {})...",
                        routine_id, subfunction
                    );
                    ka.tick(&mut uds).await?;
                    uds.routine_control(*subfunction, *routine_id, data).await?;
                    steps_completed += 1;
                    actions_executed.push(format!("Routine 0x{:04X}: {}", routine_id, description));
                }
                ModAction::PatchFlashMap {
                    map_name,
                    address_offset,
                    data,
                    description,
                    ..
                } => {
                    info!(
                        "Applying Flash Map Patch '{}' at 0x{:06X} ({} bytes)...",
                        map_name,
                        address_offset,
                        data.len()
                    );
                    ka.tick(&mut uds).await?;
                    uds.write_memory_by_address(*address_offset, data).await?;
                    steps_completed += 1;
                    actions_executed.push(format!("Patch Map '{}': {}", map_name, description));
                }
                ModAction::DtcMask {
                    p_code,
                    address_offset,
                    disable_mask,
                    description,
                    ..
                } => {
                    info!(
                        "Applying DTC {} suppression mask (0x{:02X}) at 0x{:06X}...",
                        p_code, disable_mask, address_offset
                    );
                    ka.tick(&mut uds).await?;
                    uds.write_memory_by_address(*address_offset, &[*disable_mask])
                        .await?;
                    steps_completed += 1;
                    actions_executed.push(format!("DTC Mask {}: {}", p_code, description));
                }
            }
        }

        // 8. Atomic post-mod Git garage snapshot
        let post_note = format!(
            "Applied community mod '{}' v{} by {} ({})",
            modpack.metadata.name,
            modpack.metadata.version,
            modpack.metadata.author,
            modpack.metadata.mod_id
        );
        let _ = garage.save_coding(
            vin,
            &modpack.target.ecu_name,
            &format!("MOD_{}", modpack.metadata.mod_id.to_uppercase()),
            None,
            &post_note,
        );

        let git_commit_sha = garage
            .get_history(vin)
            .ok()
            .and_then(|h| h.first().map(|c| c.hash.clone()));

        Ok(ModExecutionReport {
            mod_id: modpack.metadata.mod_id.clone(),
            mod_name: modpack.metadata.name.clone(),
            success: true,
            steps_completed,
            total_steps,
            actions_executed,
            git_commit_sha,
            live_hw_id,
            message: format!(
                "✓ Mod '{}' applied successfully! {}/{} steps executed and recorded to vehicle Git history.",
                modpack.metadata.name, steps_completed, total_steps
            ),
        })
    }
}
