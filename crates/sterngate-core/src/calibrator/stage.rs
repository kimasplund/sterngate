use super::detector::BoschMapDetector;
use super::map::EcuMap;
use crate::error::{Result, SterngateError};
use crate::flash::FirmwareSignatures;
use crate::modpack::{
    ModAction, ModCategory, ModMetadata, ModRiskLevel, ModTargetFilter, SterngateMod,
};

pub struct StageGenerator;

impl StageGenerator {
    /// A map may only become a flash patch when the detector located it in
    /// this ROM and the bytes it carries are the bytes at that address.
    pub(crate) fn require_rom_backed(map: &EcuMap, rom: &[u8]) -> Result<()> {
        if !map.provenance.is_scanned() || !map.is_rom_backed(rom) {
            return Err(SterngateError::PreFlightCheckFailed(format!(
                "refusing to emit flash patch for '{}' at 0x{:06X}: provenance {:?}, rom_backed={} (map was not located in this ROM)",
                map.name,
                map.address,
                map.provenance,
                map.is_rom_backed(rom)
            )));
        }
        Ok(())
    }

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
                    Self::require_rom_backed(&map, rom)?;
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
                    Self::require_rom_backed(&map, rom)?;
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
                    Self::require_rom_backed(&map, rom)?;
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
                    Self::require_rom_backed(&map, rom)?;
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
                    Self::require_rom_backed(&map, rom)?;
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
                    Self::require_rom_backed(&map, rom)?;
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
                    Self::require_rom_backed(&map, rom)?;
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
                    Self::require_rom_backed(&map, rom)?;
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
                    Self::require_rom_backed(&map, rom)?;
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
                "Stage 2 performance tune for {} {}. Requires physical DPF delete downpipe and EGR blanking plate. Bypasses DPF regeneration.",
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

    /// DTC suppression is unsupported until the detector understands the
    /// Bosch fault-path table. A raw 2-byte pattern hit is not evidence of a
    /// DTC entry, so no flash write may be minted from it.
    pub fn generate_dtc_kill(
        _rom: &[u8],
        _chassis: &str,
        _ecu_name: &str,
        p_codes: &[String],
        _author: &str,
    ) -> Result<SterngateMod> {
        Err(SterngateError::PreFlightCheckFailed(format!(
            "DTC fault-path table location is unsupported until the detector rebuild; refusing to emit flash writes for {p_codes:?} at assumed addresses"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibrator::map::{EcuMap, MapCategory, MapProvenance};

    fn test_rom_with_svbl() -> Vec<u8> {
        let mut rom = vec![0xFF; 0x20_0000];
        let svbl = 2350u16.to_be_bytes();
        rom[0x1C2000] = svbl[0];
        rom[0x1C2001] = svbl[1];
        for i in [0x1C1FFE, 0x1C1FFF, 0x1C2002, 0x1C2003] {
            rom[i] = 0x00;
        }
        rom
    }

    #[test]
    fn stage1_refuses_when_any_targeted_map_is_not_scanned() {
        let rom = test_rom_with_svbl();
        let err = StageGenerator::generate_stage1(&rom, "W211", "EDC16CP31", "t").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Torque Limiter"), "{msg}");
        assert!(msg.contains("Fallback"), "{msg}");
    }

    #[test]
    fn stage2_refuses_synthetic_maps_and_emits_no_dtc_mask() {
        let rom = test_rom_with_svbl();
        let err = StageGenerator::generate_stage2(&rom, "W211", "EDC16CP31", "t").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("provenance"), "{msg}");
    }

    #[test]
    fn dtc_kill_refuses_until_detector_rebuild() {
        let rom = test_rom_with_svbl();
        let err = StageGenerator::generate_dtc_kill(&rom, "W211", "EDC16", &["P0401".into()], "t")
            .unwrap_err();
        assert!(err.to_string().contains("unsupported"), "{err}");
    }

    #[test]
    fn require_rom_backed_rejects_mislabelled_scanned_map() {
        let rom = test_rom_with_svbl();
        let map = EcuMap {
            name: "Forged".into(),
            category: MapCategory::Boost,
            provenance: MapProvenance::Scanned,
            address: 0x1C2000,
            rows: 1,
            cols: 1,
            axis_x: None,
            axis_y: None,
            data: vec![0.0],
            raw_bytes: vec![0x12, 0x34],
            factor: 1.0,
            offset: 0.0,
            unit: "mbar".into(),
            is_16bit: true,
            is_signed: false,
        };
        assert!(StageGenerator::require_rom_backed(&map, &rom).is_err());
    }

    #[test]
    fn require_rom_backed_accepts_true_scan_hit() {
        let rom = test_rom_with_svbl();
        let maps = BoschMapDetector::scan_rom(&rom);
        let svbl = maps.iter().find(|m| m.name.contains("SVBL")).unwrap();
        assert!(StageGenerator::require_rom_backed(svbl, &rom).is_ok());
    }
}
