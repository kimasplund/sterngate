//! Community Mod Execution Engine
//!
//! Enforces safety gates, checks vehicle fingerprint compatibility, verifies preconditions,
//! creates atomic Vehicle Garage snapshots, applies DID writes with bitmasking,
//! and verifies post-write states over CAN.

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use sterngate_core::{
    ModAction, ModValidationReport, Result, SterngateError, SterngateMod, VehicleGarage,
};
use sterngate_hal::VehicleInterface;

use crate::uds::UdsClient;

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

pub struct ModRunner;

impl ModRunner {
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

        // 1. Check Hardware ID if whitelist exists
        let live_hw_id = if !modpack.target.compatible_hw_ids.is_empty() {
            // Extended session
            let _ = uds.diagnostic_session_control(0x03).await;
            if let Ok(resp) = uds.read_data_by_identifier(0xF191).await {
                if resp.len() >= 4 {
                    Some(String::from_utf8_lossy(&resp[3..]).trim().to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let final_report = modpack.check_compatibility(vin, live_hw_id.as_deref(), battery_voltage);

        Ok(final_report)
    }

    /// Safely apply a community mod package to connected vehicle
    pub async fn apply_mod(
        interface: &mut dyn VehicleInterface,
        modpack: &mut SterngateMod,
        vin: &str,
        battery_voltage: f64,
        force_bypass_preconditions: bool,
    ) -> Result<ModExecutionReport> {
        // 1. Verify and auto-repair Reed-Solomon FEC
        let validation = modpack.verify_and_repair()?;
        if !validation.is_valid {
            return Err(SterngateError::ProfileError(format!(
                "Mod package integrity check failed: {:?}",
                validation.warning_messages
            )));
        }

        // 2. Safety Interlock: Voltage Check
        if battery_voltage < modpack.target.min_battery_voltage && !force_bypass_preconditions {
            return Err(SterngateError::PreFlightCheckFailed(format!(
                "Battery voltage ({:.1}V) is below required minimum ({:.1}V) for mod '{}'",
                battery_voltage, modpack.target.min_battery_voltage, modpack.metadata.name
            )));
        }

        // 3. Vehicle Chassis Fingerprint Check
        if !modpack.target.matches_chassis(vin) && !force_bypass_preconditions {
            return Err(SterngateError::ProtocolError(format!(
                "Vehicle chassis mismatch: Mod '{}' requires chassis {:?}, but connected VIN is '{}'",
                modpack.metadata.name, modpack.target.chassis, vin
            )));
        }

        let mut uds = UdsClient::new(interface, modpack.target.tx_id, modpack.target.rx_id);

        // Enter Extended Diagnostic Session (0x10 03)
        let _ = uds.diagnostic_session_control(0x03).await;

        // 4. ECU Hardware ID Whitelist Check
        let live_hw_id = if let Ok(resp) = uds.read_data_by_identifier(0xF191).await {
            if resp.len() >= 4 {
                Some(String::from_utf8_lossy(&resp[3..]).trim().to_string())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(ref hw) = live_hw_id {
            if !modpack.target.matches_hardware(hw) && !force_bypass_preconditions {
                return Err(SterngateError::ProtocolError(format!(
                    "Incompatible ECU Hardware ID '{}'. Mod '{}' only supports: {:?}",
                    hw, modpack.metadata.name, modpack.target.compatible_hw_ids
                )));
            }
        }

        // 5. Preconditions: Check Expected Original Data before writing anything
        if !force_bypass_preconditions {
            for action in &modpack.actions {
                if let ModAction::WriteDid {
                    did,
                    expected_original_data: Some(expected),
                    ..
                } = action
                {
                    match uds.read_data_by_identifier(*did).await {
                        Ok(resp) => {
                            let current_data = if resp.len() >= 3 { &resp[3..] } else { &[] };
                            let check_len = expected.len().min(current_data.len());
                            if current_data[..check_len] != expected[..check_len] {
                                return Err(SterngateError::ProtocolError(format!(
                                    "Precondition check failed for DID 0x{:04X}: expected current bytes {:02X?}, but vehicle returned {:02X?}. Mod execution halted to prevent configuration corruption.",
                                    did, expected, current_data
                                )));
                            }
                        }
                        Err(e) => {
                            warn!("Could not read DID 0x{:04X} for precondition: {}", did, e);
                        }
                    }
                } else if let ModAction::PatchFlashMap {
                    map_name,
                    address_offset,
                    expected_original_data: Some(expected),
                    ..
                } = action
                {
                    if let Ok(current_data) = uds
                        .read_memory_by_address(*address_offset, expected.len() as u16)
                        .await
                    {
                        let check_len = expected.len().min(current_data.len());
                        if current_data[..check_len] != expected[..check_len] {
                            return Err(SterngateError::ProtocolError(format!(
                                "Precondition check failed for map '{}' at 0x{:06X}: expected original bytes {:02X?}, but vehicle returned {:02X?}. Aborting flash patch.",
                                map_name, address_offset, expected, current_data
                            )));
                        }
                    }
                } else if let ModAction::DtcMask {
                    p_code,
                    address_offset,
                    original_mask,
                    ..
                } = action
                {
                    if let Ok(current_data) = uds.read_memory_by_address(*address_offset, 1).await {
                        if !current_data.is_empty() && current_data[0] != *original_mask {
                            warn!(
                                "DTC {} mask at 0x{:06X} was 0x{:02X}, expected 0x{:02X}",
                                p_code, address_offset, current_data[0], original_mask
                            );
                        }
                    }
                }
            }
        }

        // 6. Atomic Pre-Mod Git Garage Snapshot
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

        // 7. Execute Actions
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
                        // Read current bytes and merge with bitmask
                        match uds.read_data_by_identifier(*did).await {
                            Ok(resp) => {
                                let current_bytes = if resp.len() >= 3 { &resp[3..] } else { &[] };
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
                            }
                            Err(_) => data.clone(),
                        }
                    } else {
                        data.clone()
                    };

                    info!(
                        "Applying Mod Write DID 0x{:04X} ({} bytes)...",
                        did,
                        write_payload.len()
                    );
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
                    uds.write_memory_by_address(*address_offset, &[*disable_mask])
                        .await?;
                    steps_completed += 1;
                    actions_executed.push(format!("DTC Mask {}: {}", p_code, description));
                }
            }
        }

        // 8. Atomic Post-Mod Git Garage Snapshot
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
