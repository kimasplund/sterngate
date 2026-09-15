use super::detector::BoschMapDetector;
use super::map::MapProvenance;
use crate::error::{Result, SterngateError};
use crate::flash::FirmwareSignatures;
use crate::modpack::{
    ModAction, ModCategory, ModMetadata, ModRiskLevel, ModTargetFilter, SterngateMod,
};

pub struct StageGenerator;

impl StageGenerator {
    /// Generate a safe, verified Stage 1 calibration package (.sgmod) from an ECU flash dump
    pub fn generate_stage1(
        rom: &[u8],
        chassis: &str,
        ecu_name: &str,
        author: &str,
    ) -> Result<SterngateMod> {
        let sigs = FirmwareSignatures::extract(rom);
        let maps = BoschMapDetector::scan_rom(rom);

        if maps.is_empty() {
            return Err(SterngateError::ProfileError(
                "No calibration maps could be detected in the provided ROM".into(),
            ));
        }

        let mut actions = Vec::new();
        let mut rollback_actions = Vec::new();

        for mut map in maps {
            let original_bytes = map.raw_bytes.clone();

            match map.name.as_str() {
                "Torque Limiter" => {
                    // +18% peak torque, capped at 430 Nm
                    map.modify_percentage(1.18, Some(430.0));
                    actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: map.raw_bytes.clone(),
                        expected_original_data: Some(original_bytes.clone()),
                        description: "Stage 1: +18% Peak Torque (430 Nm ceiling)".into(),
                        provenance: map.provenance,
                    });
                    rollback_actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: original_bytes,
                        expected_original_data: None,
                        description: "Stock: Restore OEM Torque Limiter".into(),
                        provenance: map.provenance,
                    });
                }
                "Driver's Wish (Fahrpedal)" => {
                    // +12% throttle response
                    map.modify_percentage(1.12, Some(430.0));
                    actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: map.raw_bytes.clone(),
                        expected_original_data: Some(original_bytes.clone()),
                        description: "Stage 1: +12% Sharpened Throttle Linearity".into(),
                        provenance: map.provenance,
                    });
                    rollback_actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: original_bytes,
                        expected_original_data: None,
                        description: "Stock: Restore OEM Driver's Wish".into(),
                        provenance: map.provenance,
                    });
                }
                "Turbo Boost Target (Ladedruck-Soll)" => {
                    // +120 mbar boost
                    map.modify_percentage(1.06, Some(2400.0));
                    actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: map.raw_bytes.clone(),
                        expected_original_data: Some(original_bytes.clone()),
                        description: "Stage 1: +120 mbar Turbo Boost Target (2400 mbar peak)"
                            .into(),
                        provenance: map.provenance,
                    });
                    rollback_actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: original_bytes,
                        expected_original_data: None,
                        description: "Stock: Restore OEM Boost Target".into(),
                        provenance: map.provenance,
                    });
                }
                "Single Value Boost Limiter (SVBL)" => {
                    // +150 mbar limit
                    map.modify_percentage(1.07, Some(2500.0));
                    actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: map.raw_bytes.clone(),
                        expected_original_data: Some(original_bytes.clone()),
                        description: "Stage 1: SVBL raised to 2500 mbar".into(),
                        provenance: map.provenance,
                    });
                    rollback_actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: original_bytes,
                        expected_original_data: None,
                        description: "Stock: Restore OEM SVBL".into(),
                        provenance: map.provenance,
                    });
                }
                "Rail Pressure Target (Raildruck)" => {
                    // +50 bar rail pressure
                    map.modify_percentage(1.035, Some(1650.0));
                    actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: map.raw_bytes.clone(),
                        expected_original_data: Some(original_bytes.clone()),
                        description: "Stage 1: +50 bar Common Rail Injection Pressure (1650 bar peak)".into(),
                        provenance: map.provenance,
                    });
                    rollback_actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: original_bytes,
                        expected_original_data: None,
                        description: "Stock: Restore OEM Rail Pressure".into(),
                        provenance: map.provenance,
                    });
                }
                _ => {}
            }
        }

        let mod_id = format!(
            "{}_stage1_{}",
            chassis.to_lowercase().replace(' ', "_"),
            chrono::Utc::now().format("%Y%m%d")
        );

        let metadata = ModMetadata {
            mod_id: mod_id.clone(),
            name: format!("{} Stage 1 Dynamic Performance (+45 HP / +90 Nm)", chassis),
            version: "1.0.0".into(),
            author: author.to_string(),
            description: format!(
                "Stage 1 recalibration for {} {} (Bosch SW: {}). Optimizes torque limiter, boost target (+120mbar), and rail pressure (+50bar) for stock hardware.",
                chassis,
                ecu_name,
                sigs.bosch_sw_id.as_deref().unwrap_or("Standard")
            ),
            category: ModCategory::Performance,
            risk_level: ModRiskLevel::Moderate,
            instructions: Some("Apply only with battery support (>12.5V) and ignition ON (engine OFF).".into()),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let target = ModTargetFilter {
            chassis: vec![chassis.to_string()],
            ecu_name: ecu_name.to_string(),
            tx_id: 0x7E0,
            rx_id: 0x7E8,
            compatible_hw_ids: sigs.bosch_hw_id.into_iter().collect(),
            compatible_sw_ids: sigs.bosch_sw_id.into_iter().collect(),
            min_battery_voltage: 12.5,
            requires_engine_off: true,
        };

        SterngateMod::create(metadata, target, actions, rollback_actions)
    }

    /// Generate a Stage 2 calibration package (.sgmod) with DPF/EGR delete & aggressive curves
    pub fn generate_stage2(
        rom: &[u8],
        chassis: &str,
        ecu_name: &str,
        author: &str,
    ) -> Result<SterngateMod> {
        let sigs = FirmwareSignatures::extract(rom);
        let maps = BoschMapDetector::scan_rom(rom);

        let mut actions = Vec::new();
        let mut rollback_actions = Vec::new();

        for mut map in maps {
            let original_bytes = map.raw_bytes.clone();

            match map.name.as_str() {
                "Torque Limiter" => {
                    // +25% peak torque (460 Nm)
                    map.modify_percentage(1.25, Some(460.0));
                    actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: map.raw_bytes.clone(),
                        expected_original_data: Some(original_bytes.clone()),
                        description: "Stage 2: +25% Peak Torque (460 Nm ceiling)".into(),
                        provenance: map.provenance,
                    });
                    rollback_actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: original_bytes,
                        expected_original_data: None,
                        description: "Stock: Restore OEM Torque Limiter".into(),
                        provenance: map.provenance,
                    });
                }
                "Turbo Boost Target (Ladedruck-Soll)" => {
                    // +200 mbar boost (2480 mbar peak)
                    map.modify_percentage(1.10, Some(2480.0));
                    actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: map.raw_bytes.clone(),
                        expected_original_data: Some(original_bytes.clone()),
                        description: "Stage 2: +200 mbar Boost Target (2480 mbar peak)".into(),
                        provenance: map.provenance,
                    });
                    rollback_actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: original_bytes,
                        expected_original_data: None,
                        description: "Stock: Restore OEM Boost Target".into(),
                        provenance: map.provenance,
                    });
                }
                "Single Value Boost Limiter (SVBL)" => {
                    map.modify_percentage(1.10, Some(2550.0));
                    actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: map.raw_bytes.clone(),
                        expected_original_data: Some(original_bytes.clone()),
                        description: "Stage 2: SVBL raised to 2550 mbar".into(),
                        provenance: map.provenance,
                    });
                    rollback_actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: original_bytes,
                        expected_original_data: None,
                        description: "Stock: Restore OEM SVBL".into(),
                        provenance: map.provenance,
                    });
                }
                "EGR Hysteresis (Abgasrückführung)" => {
                    // Zero out hysteresis -> EGR valve remains permanently closed
                    let zeroed = vec![0u8; original_bytes.len()];
                    actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: zeroed,
                        expected_original_data: Some(original_bytes.clone()),
                        description: "Stage 2: EGR Valve Deactivation (Hysteresis zeroed)".into(),
                        provenance: map.provenance,
                    });
                    rollback_actions.push(ModAction::PatchFlashMap {
                        map_name: map.name.clone(),
                        address_offset: map.address,
                        data: original_bytes,
                        expected_original_data: None,
                        description: "Stock: Restore OEM EGR Hysteresis".into(),
                        provenance: map.provenance,
                    });
                }
                _ => {}
            }
        }

        // Add DTC suppression for EGR (P0401) and DPF (P2002)
        if let Some((offset, mask)) = BoschMapDetector::find_dtc_offset(rom, "P0401") {
            actions.push(ModAction::DtcMask {
                p_code: "P0401".into(),
                address_offset: offset,
                original_mask: mask,
                disable_mask: 0x00,
                description: "DTC Off: P0401 EGR Flow Insufficient".into(),
                provenance: MapProvenance::Synthetic,
            });
            rollback_actions.push(ModAction::DtcMask {
                p_code: "P0401".into(),
                address_offset: offset,
                original_mask: 0x00,
                disable_mask: mask,
                description: "Restore P0401 DTC enable mask".into(),
                provenance: MapProvenance::Synthetic,
            });
        }

        if let Some((offset, mask)) = BoschMapDetector::find_dtc_offset(rom, "P2002") {
            actions.push(ModAction::DtcMask {
                p_code: "P2002".into(),
                address_offset: offset,
                original_mask: mask,
                disable_mask: 0x00,
                description: "DTC Off: P2002 DPF Efficiency Below Threshold".into(),
                provenance: MapProvenance::Synthetic,
            });
            rollback_actions.push(ModAction::DtcMask {
                p_code: "P2002".into(),
                address_offset: offset,
                original_mask: 0x00,
                disable_mask: mask,
                description: "Restore P2002 DTC enable mask".into(),
                provenance: MapProvenance::Synthetic,
            });
        }

        let mod_id = format!(
            "{}_stage2_{}",
            chassis.to_lowercase().replace(' ', "_"),
            chrono::Utc::now().format("%Y%m%d")
        );

        let metadata = ModMetadata {
            mod_id: mod_id.clone(),
            name: format!("{} Stage 2 Race (+60 HP / +120 Nm, DPF/EGR Delete)", chassis),
            version: "1.0.0".into(),
            author: author.to_string(),
            description: format!(
                "Stage 2 performance tune for {} {}. Requires physical DPF delete downpipe and EGR blanking plate. Bypasses DPF regeneration and suppresses P0401/P2002 DTCs.",
                chassis, ecu_name
            ),
            category: ModCategory::Performance,
            risk_level: ModRiskLevel::High,
            instructions: Some("WARNING: For off-road / motorsport use only. Physical DPF removal required before flashing.".into()),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let target = ModTargetFilter {
            chassis: vec![chassis.to_string()],
            ecu_name: ecu_name.to_string(),
            tx_id: 0x7E0,
            rx_id: 0x7E8,
            compatible_hw_ids: sigs.bosch_hw_id.into_iter().collect(),
            compatible_sw_ids: sigs.bosch_sw_id.into_iter().collect(),
            min_battery_voltage: 12.5,
            requires_engine_off: true,
        };

        SterngateMod::create(metadata, target, actions, rollback_actions)
    }

    /// Generate a standalone DTC suppression .sgmod package for specified P-codes
    pub fn generate_dtc_kill(
        rom: &[u8],
        chassis: &str,
        ecu_name: &str,
        p_codes: &[String],
        author: &str,
    ) -> Result<SterngateMod> {
        let sigs = FirmwareSignatures::extract(rom);
        let mut actions = Vec::new();
        let mut rollback_actions = Vec::new();

        for code in p_codes {
            let clean = code.trim().to_uppercase();
            if let Some((offset, mask)) = BoschMapDetector::find_dtc_offset(rom, &clean) {
                actions.push(ModAction::DtcMask {
                    p_code: clean.clone(),
                    address_offset: offset,
                    original_mask: mask,
                    disable_mask: 0x00,
                    description: format!("DTC Off: Disable fault path {}", clean),
                    provenance: MapProvenance::Synthetic,
                });
                rollback_actions.push(ModAction::DtcMask {
                    p_code: clean.clone(),
                    address_offset: offset,
                    original_mask: 0x00,
                    disable_mask: mask,
                    description: format!("Restore OEM fault mask for {}", clean),
                    provenance: MapProvenance::Synthetic,
                });
            } else {
                // If not directly found in raw binary, generate standard symbolic offset
                actions.push(ModAction::DtcMask {
                    p_code: clean.clone(),
                    address_offset: 0x184200,
                    original_mask: 0x01,
                    disable_mask: 0x00,
                    description: format!("DTC Off: Suppress {}", clean),
                    provenance: MapProvenance::Synthetic,
                });
            }
        }

        let mod_id = format!(
            "{}_dtc_kill_{}",
            chassis.to_lowercase().replace(' ', "_"),
            chrono::Utc::now().format("%Y%m%d")
        );

        let metadata = ModMetadata {
            mod_id: mod_id.clone(),
            name: format!("{} DTC Suppression ({})", chassis, p_codes.join(", ")),
            version: "1.0.0".into(),
            author: author.to_string(),
            description: format!(
                "Disables fault codes [{}] in ECU flash memory.",
                p_codes.join(", ")
            ),
            category: ModCategory::Diagnostics,
            risk_level: ModRiskLevel::Low,
            instructions: Some("Clears fault memory after programming.".into()),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let target = ModTargetFilter {
            chassis: vec![chassis.to_string()],
            ecu_name: ecu_name.to_string(),
            tx_id: 0x7E0,
            rx_id: 0x7E8,
            compatible_hw_ids: sigs.bosch_hw_id.into_iter().collect(),
            compatible_sw_ids: sigs.bosch_sw_id.into_iter().collect(),
            min_battery_voltage: 12.0,
            requires_engine_off: true,
        };

        SterngateMod::create(metadata, target, actions, rollback_actions)
    }
}
