pub mod discoverer;
pub mod flasher;
pub mod gate;
pub mod importer;
pub mod isotp;
pub mod kwp2000;
pub mod modrunner;
pub mod scanner;
pub mod seedkey;
pub mod service;
pub mod uds;

pub use discoverer::BusDiscoverer;
pub use flasher::FlashingWorker;
pub use gate::TransactionGate;
pub use importer::{ImportReport, ProfileImporter};
pub use isotp::IsoTpChannel;
pub use kwp2000::KwpClient;
pub use modrunner::{ModExecutionReport, ModRunner, TargetFingerprintPolicy};
pub use scanner::{ModuleScanResult, VehicleDiagnosticReport, VehicleScanner};
pub use seedkey::{DaimlerSeedKey, DaimlerSolver, SeedKeySolver};
pub use service::{ServiceRoutineManager, VinAdaptationManager};
pub use uds::UdsClient;

#[cfg(test)]
mod tests {
    use super::*;
    use sterngate_core::Language;
    use sterngate_core::{
        MapProvenance, ModAction, ModCategory, ModMetadata, ModRiskLevel, ModTargetFilter,
        SterngateError, SterngateMod,
    };
    use sterngate_hal::{VehicleInterface, VirtualCanInterface};

    #[test]
    fn test_daimler_seed_key_solvers() {
        let seed = [0x12, 0x34, 0x56, 0x78];
        let key_lvl1 = DaimlerSeedKey::calculate_level1(&seed).unwrap();
        assert_ne!(key_lvl1, [0, 0, 0, 0]);

        let key_lvl3 = DaimlerSeedKey::calculate_level3(&seed).unwrap();
        assert_ne!(key_lvl3, [0, 0, 0, 0]);

        let key_lvl0b = DaimlerSeedKey::calculate_level0b(&seed).unwrap();
        assert_ne!(key_lvl0b, [0, 0, 0, 0]);

        let unlocked = DaimlerSeedKey::calculate_level1(&[0, 0, 0, 0]).unwrap();
        assert_eq!(unlocked, [0, 0, 0, 0]);
    }

    #[tokio::test]
    async fn test_uds_client_read_did() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        let mut uds = UdsClient::new(&mut sim, 0x7E0, 0x7E8);
        let resp = uds.read_data_by_identifier(0x0105).await.unwrap();
        assert_eq!(resp[0], 0x62);
        assert_eq!(resp[1], 0x01);
        assert_eq!(resp[2], 0x05);
        assert_eq!(resp[3], 0x80); // 128 - 40 = 88°C
    }

    #[tokio::test]
    async fn test_flasher_preflight_battery_interlock() {
        use sha2::Digest;

        let flasher = FlashingWorker::new();
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        let dummy_rom = vec![0x11, 0x22, 0x33, 0x44];
        let mut hasher = sha2::Sha256::new();
        Digest::update(&mut hasher, &dummy_rom);
        let sha256_checksum = format!("{:x}", Digest::finalize(hasher));
        let crc32_checksum = crc32fast::hash(&dummy_rom);

        let manifest = sterngate_core::FlashPackageManifest {
            target_module: "EDC16".into(),
            expected_hw_id: "0281012224".into(),
            expected_sw_id: "1037372332".into(),
            sha256_checksum,
            crc32_checksum,
            flash_start_address: 0x00040000,
            flash_length: dummy_rom.len() as u32,
            block_size: 256,
        };

        // Battery voltage 11.8V -> MUST FAIL!
        let report_low_voltage = flasher
            .run_preflight_checks(&manifest, &dummy_rom, 11.8, &mut sim)
            .await
            .unwrap();
        assert!(!report_low_voltage.passed);

        // Battery voltage 13.5V -> MUST PASS!
        let report_good_voltage = flasher
            .run_preflight_checks(&manifest, &dummy_rom, 13.5, &mut sim)
            .await
            .unwrap();
        assert!(report_good_voltage.passed);
    }

    #[tokio::test]
    async fn test_transaction_gate_verification() {
        use sterngate_core::{CommandEnvelope, VehicleProfile};

        let gate = TransactionGate::new();
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();

        // 1. Valid envelope targeting 722.6 transmission fluid temp (0x2001, 1 byte)
        let valid_env = CommandEnvelope::new("EGS52", 0x2E, Some(0x2001), vec![0x78]);
        let report = gate
            .verify_and_authorize(&valid_env, &profile)
            .await
            .unwrap();
        assert!(report.is_valid);

        // 2. Replay with same idempotency key -> MUST FAIL
        let duplicate = valid_env.clone();
        assert!(gate
            .verify_and_authorize(&duplicate, &profile)
            .await
            .is_err());

        // 3. Schema mismatch: DID 0x2001 expects 1 byte, send 3 bytes -> MUST FAIL
        let invalid_schema =
            CommandEnvelope::new("EGS52", 0x2E, Some(0x2001), vec![0x01, 0x02, 0x03]);
        assert!(gate
            .verify_and_authorize(&invalid_schema, &profile)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_uds_client_routine_control() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        let mut uds = UdsClient::new(&mut sim, 0x7E0, 0x7E8);
        // Start Fuel Pump Prime & Rail Bleed (0xFF01)
        let resp = uds.routine_control(0x01, 0xFF01, &[]).await.unwrap();
        assert_eq!(resp[0], 0x71); // Positive response
        assert_eq!(resp[1], 0x01); // startRoutine
        assert_eq!(resp[2], 0xFF); // Routine ID high
        assert_eq!(resp[3], 0x01); // Routine ID low
        assert_eq!(resp[4], 0x00); // Status OK
    }

    #[tokio::test]
    async fn test_vehicle_scanner_quick_test() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        let report = VehicleScanner::scan(&mut sim, Language::En).await.unwrap();
        assert!(!report.vin.is_empty());
        assert_eq!(report.decoded.manufacturer, "Mercedes-Benz");
        assert_eq!(report.decoded.model_name, "E 220 T CDI");
        assert!(report.modules_responding >= 4);

        // Verify Markdown rendering
        let md = report.to_markdown(Language::En);
        assert!(md.contains("E 220 T CDI"));
        assert!(md.contains("VIN"));

        // Verify conversion to VehicleRecord
        let rec = report.to_vehicle_record();
        assert_eq!(rec.vin, report.vin);
        assert!(!rec.detected_modules.is_empty());
    }

    #[tokio::test]
    async fn test_control_suspension_compressor() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // 1. Inhibit (Safe mode)
        let res_inhibit = VehicleScanner::control_suspension_compressor(&mut sim, "inhibit")
            .await
            .unwrap();
        assert!(res_inhibit.contains("0x0210"));
        assert!(res_inhibit.contains("Inhibit"));

        // 2. Workshop / Transport mode
        let res_workshop = VehicleScanner::control_suspension_compressor(&mut sim, "workshop")
            .await
            .unwrap();
        assert!(res_workshop.contains("0x0211"));

        // 3. Restore normal
        let res_restore = VehicleScanner::control_suspension_compressor(&mut sim, "restore")
            .await
            .unwrap();
        assert!(res_restore.contains("0x0212"));
    }

    #[tokio::test]
    async fn test_bus_discovery_and_profile_generation() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // Discover ECUs across 0x7E0..=0x7E2
        let discovered = BusDiscoverer::discover_ecus(&mut sim, 0x7E0..=0x7E2, 20, None)
            .await
            .unwrap();

        assert!(!discovered.is_empty());
        let edc = discovered.iter().find(|e| e.tx_id == 0x7E0).unwrap();
        assert_eq!(edc.rx_id, 0x7E8);
        assert!(edc.part_number.is_some());

        // Generate profile
        let profile = BusDiscoverer::generate_profile(
            &discovered,
            "Mercedes-Benz",
            "W211",
            "discovered_w211",
        );
        assert_eq!(profile.oem, "Mercedes-Benz");
        assert!(!profile.modules.is_empty());
    }

    #[tokio::test]
    async fn test_workshop_service_routines() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // 1. SBC Deactivation
        let sbc_deact = ServiceRoutineManager::deactivate_sbc(&mut sim, 0x7E2, 0x7EA)
            .await
            .unwrap();
        assert!(sbc_deact.success);
        assert_eq!(sbc_deact.accumulator_pressure_bar, 0.0);
        assert!(sbc_deact.wake_up_suppressed);

        // 2. SBC Reactivation
        let sbc_react = ServiceRoutineManager::reactivate_sbc(&mut sim, 0x7E2, 0x7EA)
            .await
            .unwrap();
        assert!(sbc_react.success);
        assert_eq!(sbc_react.accumulator_pressure_bar, 158.0);
        assert!(!sbc_react.wake_up_suppressed);

        // 3. Common Rail IMA Coding (Read & Write)
        let ima_read = ServiceRoutineManager::read_injector_ima(&mut sim, 0x7E0, 0x7E8, 1)
            .await
            .unwrap();
        assert_eq!(ima_read.cylinder, 1);
        assert!(!ima_read.code.is_empty());

        let ima_write =
            ServiceRoutineManager::write_injector_ima(&mut sim, 0x7E0, 0x7E8, 2, "A8B12F")
                .await
                .unwrap();
        assert_eq!(ima_write.cylinder, 2);
        assert_eq!(ima_write.code, "A8B12F");

        // 4. Air suspension corner actuation
        use sterngate_core::{EcoStartStopMode, SuspensionCorner, SuspensionCornerAction};
        let act = ServiceRoutineManager::actuate_suspension_corner(
            &mut sim,
            0x7E4,
            0x7EC,
            SuspensionCorner::RearLeft,
            SuspensionCornerAction::Inflate,
        )
        .await
        .unwrap();
        assert!(act.contains("Rear-Left"));
        assert!(act.contains("0x0213"));

        // 5. AdBlue / SCR 800km Emergency Countdown & Lockout Reset
        let adblue = ServiceRoutineManager::reset_adblue_countdown(&mut sim, 0x7E0, 0x7E8)
            .await
            .unwrap();
        assert!(adblue.success);
        assert!(adblue.countdown_reset);
        assert!(adblue.level_sensor_calibrated);

        // 6. ECO Start-Stop Memory Mode Configuration
        let eco = ServiceRoutineManager::configure_eco_start_stop(
            &mut sim,
            0x7E0,
            0x7E8,
            EcoStartStopMode::RememberLastState,
        )
        .await
        .unwrap();
        assert!(eco.success);
        assert_eq!(eco.mode, EcoStartStopMode::RememberLastState);

        // 7. EGR Soot Optimization Relearn
        let egr = ServiceRoutineManager::optimize_egr_adaptation(&mut sim, 0x7E0, 0x7E8)
            .await
            .unwrap();
        assert!(egr.success);
        assert_eq!(egr.air_mass_offset_mg, 40.0);
        assert!(egr.stops_relearned);
    }

    #[tokio::test]
    async fn test_html_diagnostic_report_export() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        let report = VehicleScanner::scan(&mut sim, Language::En).await.unwrap();
        let html = report.to_html(Language::En);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Sterngate Automotive Diagnostic Health Report"));
        assert!(html.contains(&report.vin));
        assert!(html.contains("System Voltage"));
    }

    #[tokio::test]
    async fn test_rom_signature_inspection_and_hardware_matching() {
        use sterngate_core::RomCompatibilityVerdict;

        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        let flasher = FlashingWorker::new();

        // 1. Test Matching ROM (Bosch HW 0281012224 matches virtual EDC16 HW)
        let mut match_rom = vec![0xEA; 4096];
        let hw_match = b"0281012224";
        let sw_match = b"1037372332";
        match_rom[64..64 + hw_match.len()].copy_from_slice(hw_match);
        match_rom[128..128 + sw_match.len()].copy_from_slice(sw_match);

        let report_match = flasher
            .inspect_rom(&mut sim, 0x7E0, 0x7E8, &match_rom)
            .await
            .unwrap();

        assert!(report_match.can_flash);
        assert_eq!(
            report_match.signatures.bosch_hw_id.as_deref(),
            Some("0281012224")
        );
        assert_ne!(
            report_match.verdict,
            RomCompatibilityVerdict::HardwareMismatch
        );

        // 2. Test Mismatched ROM (Bosch HW 0281013345 does NOT match virtual EDC16 HW 0281012224)
        let mut mismatch_rom = vec![0xEA; 4096];
        let hw_mismatch = b"0281013345";
        mismatch_rom[64..64 + hw_mismatch.len()].copy_from_slice(hw_mismatch);

        let report_mismatch = flasher
            .inspect_rom(&mut sim, 0x7E0, 0x7E8, &mismatch_rom)
            .await
            .unwrap();

        assert!(!report_mismatch.can_flash);
        assert_eq!(
            report_mismatch.verdict,
            RomCompatibilityVerdict::HardwareMismatch
        );
        assert!(report_mismatch
            .risk_explanation
            .contains("CRITICAL HARDWARE MISMATCH"));
    }

    #[tokio::test]
    async fn test_quick_mod_routines() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // 1. Speed limiter (VMax -> 250 km/h)
        let v_res = ServiceRoutineManager::configure_speed_limiter(&mut sim, 0x7E0, 0x7E8, 250)
            .await
            .unwrap();
        assert!(v_res.success);
        assert_eq!(v_res.speed_limit_kmh, 250);

        // 2. Seatbelt chime mute
        let s_res = ServiceRoutineManager::configure_seatbelt_chime(&mut sim, 0x7C0, 0x7C8, false)
            .await
            .unwrap();
        assert!(s_res.success);
        assert!(!s_res.acoustic_chime_enabled);

        // 3. Tank liters display
        let t_res =
            ServiceRoutineManager::configure_tank_liters_display(&mut sim, 0x7C0, 0x7C8, true)
                .await
                .unwrap();
        assert!(t_res.success);
        assert!(t_res.exact_liters_display_enabled);

        // 4. Cornering lights
        let c_res = ServiceRoutineManager::configure_cornering_lights(&mut sim, 0x7E2, 0x7EA, true)
            .await
            .unwrap();
        assert!(c_res.success);
        assert!(c_res.cornering_lights_enabled);
    }

    #[test]
    fn test_profile_importer() {
        let temp_in = std::env::temp_dir().join("sterngate_import_test_in");
        let temp_out = std::env::temp_dir().join("sterngate_import_test_out");
        let _ = std::fs::remove_dir_all(&temp_in);
        let _ = std::fs::remove_dir_all(&temp_out);
        std::fs::create_dir_all(&temp_in).unwrap();
        std::fs::create_dir_all(&temp_out).unwrap();

        // Create a mock CBF file (e.g. CR4.cbf)
        let mock_cbf_data = vec![0x43, 0x41, 0x45, 0x53, 0x41, 0x52, 0x22, 0x01, 0x12, 0x34];
        std::fs::write(temp_in.join("CR4.cbf"), &mock_cbf_data).unwrap();

        // Create a mock SMR-D file (e.g. MED17_W205.smr-d)
        let mock_smrd_data = b"MOCK_SMRD_ODX_CONTAINER";
        std::fs::write(temp_in.join("MED17_W205.smr-d"), mock_smrd_data).unwrap();

        let report = ProfileImporter::import_from_path(&temp_in, &temp_out).unwrap();
        eprintln!("REPORT: {:?}", report);
        assert_eq!(report.cbf_files_found, 1);
        assert_eq!(report.smrd_files_found, 1);
        assert_eq!(
            report.profiles_generated.len(),
            2,
            "Warnings: {:?}",
            report.warnings
        );

        // Verify generated profile JSON files exist and load
        let gen_profile_path = temp_out.join("w164_w251_cr4.json");
        assert!(gen_profile_path.exists());
        let prof = sterngate_core::VehicleProfile::load_from_file(&gen_profile_path).unwrap();
        assert_eq!(prof.oem, "Mercedes-Benz");
        assert!(prof.modules.contains_key("CR4"));

        let gen_smrd_path = temp_out.join("w205_med17_w205.json");
        assert!(gen_smrd_path.exists());
        let prof_smrd = sterngate_core::VehicleProfile::load_from_file(&gen_smrd_path).unwrap();
        assert_eq!(prof_smrd.chassis, "W205");

        let _ = std::fs::remove_dir_all(&temp_in);
        let _ = std::fs::remove_dir_all(&temp_out);
    }

    #[tokio::test]
    async fn test_mod_runner_execution_and_safety_checks() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();

        let metadata = ModMetadata {
            mod_id: "w211-vmax-300".into(),
            name: "W211 VMax 300 km/h".into(),
            version: "1.0.0".into(),
            author: "TunerKim".into(),
            description: "Raises road speed limiter threshold to 300 km/h".into(),
            category: ModCategory::Performance,
            risk_level: ModRiskLevel::Moderate,
            instructions: Some("Engine off, ignition on".into()),
            created_at: "2026-09-14T12:00:00Z".into(),
        };

        let target = ModTargetFilter {
            chassis: vec!["W211".into(), "S211".into()],
            ecu_name: "EDC16".into(),
            tx_id: 0x7E0,
            rx_id: 0x7E8,
            compatible_hw_ids: vec![],
            compatible_sw_ids: vec![],
            min_battery_voltage: 12.0,
            requires_engine_off: true,
        };

        let actions = vec![ModAction::WriteDid {
            did: 0x0110,
            data: vec![0x01, 0x2C], // 300 km/h
            bitmask: None,
            expected_original_data: None,
            description: "Set VMax to 300".into(),
        }];

        let mut modpack = SterngateMod::create(metadata, target, actions, vec![]).unwrap();

        // 1. Inspect compatibility
        let report = ModRunner::inspect_compatibility(
            &mut iface,
            &modpack,
            Some("WDB2112061A123456"),
            Some(12.6),
        )
        .await
        .unwrap();
        assert!(report.is_valid);
        assert!(report.matched_vehicle);

        // 2. Chassis mismatch rejection
        let chassis_err = ModRunner::apply_mod(
            &mut iface,
            &mut modpack,
            "WDB2040011A999999",
            12.6,
            TargetFingerprintPolicy::Enforce,
        )
        .await;
        assert!(chassis_err.is_err());
        assert!(chassis_err
            .unwrap_err()
            .to_string()
            .contains("chassis mismatch"));

        // 3. Low voltage rejection
        let volt_err = ModRunner::apply_mod(
            &mut iface,
            &mut modpack,
            "WDB2112061A123456",
            11.5,
            TargetFingerprintPolicy::Enforce,
        )
        .await;
        assert!(volt_err.is_err());
        assert!(volt_err.unwrap_err().to_string().contains("voltage"));

        // 4. Successful execution
        let exec = ModRunner::apply_mod(
            &mut iface,
            &mut modpack,
            "WDB2112061A123456",
            12.6,
            TargetFingerprintPolicy::Enforce,
        )
        .await
        .unwrap();
        assert!(exec.success);
        assert_eq!(exec.steps_completed, 1);
        assert!(exec.git_commit_sha.is_some());
    }

    fn runner_metadata() -> ModMetadata {
        ModMetadata {
            mod_id: "w211-runner-test".into(),
            name: "W211 Runner Test".into(),
            version: "1.0.0".into(),
            author: "TunerKim".into(),
            description: "runner gate tests".into(),
            category: ModCategory::Performance,
            risk_level: ModRiskLevel::Moderate,
            instructions: None,
            created_at: "2026-09-15T12:00:00Z".into(),
        }
    }

    fn runner_target(compatible_hw_ids: Vec<String>, min_v: f64) -> ModTargetFilter {
        ModTargetFilter {
            chassis: vec!["W211".into()],
            ecu_name: "EDC16".into(),
            tx_id: 0x7E0,
            rx_id: 0x7E8,
            compatible_hw_ids,
            compatible_sw_ids: vec![],
            min_battery_voltage: min_v,
            requires_engine_off: true,
        }
    }

    fn runner_mod(actions: Vec<ModAction>, hw_ids: Vec<String>, min_v: f64) -> SterngateMod {
        SterngateMod::create(
            runner_metadata(),
            runner_target(hw_ids, min_v),
            actions,
            vec![],
        )
        .unwrap()
    }

    fn patch(address: u32, provenance: MapProvenance, expected: Option<Vec<u8>>) -> ModAction {
        ModAction::PatchFlashMap {
            map_name: "Torque Limiter".into(),
            address_offset: address,
            data: vec![0x0B, 0xB8, 0x10],
            expected_original_data: expected,
            description: "+18% Torque Limiter".into(),
            provenance,
        }
    }

    /// Recompute integrity over a mutated package (what a forger must do).
    fn resign(m: &mut SterngateMod) {
        use sha2::Digest;
        let payload =
            SterngateMod::canonical_payload_bytes(&m.target, &m.actions, &m.rollback_actions)
                .unwrap();
        m.integrity.payload_crc32 = crc32fast::hash(&payload);
        let mut hasher = sha2::Sha256::new();
        Digest::update(&mut hasher, &payload);
        m.integrity.payload_sha256 = format!("{:x}", Digest::finalize(hasher));
        m.integrity.fec_parity_bytes =
            sterngate_core::ReedSolomonCodec::default_codec().encode(&payload);
    }

    fn dtc_mask(original_mask: u8, provenance: MapProvenance) -> ModAction {
        ModAction::DtcMask {
            p_code: "P0401".into(),
            address_offset: 0x1CE000,
            original_mask,
            disable_mask: 0x00,
            description: "DTC Off: P0401".into(),
            provenance,
        }
    }

    /// DID 0x0201 is one the virtual ECU serves (`62 02 01 01`); a bitmask
    /// write must be able to read the current value first.
    fn write_did(bitmask: Option<Vec<u8>>) -> ModAction {
        ModAction::WriteDid {
            did: 0x0201,
            data: vec![0x00],
            bitmask,
            expected_original_data: None,
            description: "seatbelt chime".into(),
        }
    }

    /// Distinct per-test VIN, one per test function that might reach the Git
    /// garage snapshot: sharing one VIN across concurrently-running tests races
    /// on the same non-temp `data/vehicles/<vin>/.git/index.lock`. Keeps the
    /// `211` substring so the W211 chassis check still matches.
    fn vin(n: u8) -> String {
        format!("WDB2112061A7777{n:02}")
    }

    fn err_text(r: Result<ModExecutionReport, SterngateError>) -> String {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    #[tokio::test]
    async fn test_synthetic_provenance_refused_before_bus_traffic() {
        // Interface deliberately NOT opened: any bus exchange would surface as
        // a different error, so a provenance message proves the gate fires first.
        let mut iface = VirtualCanInterface::new();
        let mut m = runner_mod(
            vec![patch(0x1C1000, MapProvenance::Synthetic, Some(vec![0; 3]))],
            vec![],
            12.5,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(1),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("provenance"), "{msg}");

        let mut m = runner_mod(
            vec![patch(0x1C1000, MapProvenance::Unverified, Some(vec![0; 3]))],
            vec![],
            12.5,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(1),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("provenance"), "{msg}");
    }

    #[tokio::test]
    async fn test_patch_flash_map_read_failure_refuses() {
        let mut iface = VirtualCanInterface::with_failing_services(&[0x23]);
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3]))],
            vec![],
            12.5,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(2),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("original bytes"), "{msg}");
    }

    #[tokio::test]
    async fn test_patch_flash_map_without_expected_bytes_refused() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![patch(0x1C1000, MapProvenance::Scanned, None)],
            vec![],
            12.5,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(3),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("expected_original_data"), "{msg}");
    }

    #[tokio::test]
    async fn test_patch_flash_map_mismatch_refuses() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        // The mock returns zeros; the package claims the ECU holds 0B B8 10.
        let mut m = runner_mod(
            vec![patch(
                0x1C1000,
                MapProvenance::Scanned,
                Some(vec![0x0B, 0xB8, 0x10]),
            )],
            vec![],
            12.5,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(4),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("expected original bytes"), "{msg}");
    }

    #[tokio::test]
    async fn test_patch_flash_map_length_mismatch_refused() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 2]))],
            vec![],
            12.5,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(5),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("exactly"), "{msg}");
    }

    #[tokio::test]
    async fn test_dtc_mask_read_failure_and_mismatch_refuse() {
        let mut iface = VirtualCanInterface::with_failing_services(&[0x23]);
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![dtc_mask(0x00, MapProvenance::Scanned)], vec![], 12.5);
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(6),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("current mask"), "{msg}");

        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![dtc_mask(0xFF, MapProvenance::Scanned)], vec![], 12.5);
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(6),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("expected 0xFF"), "{msg}");
    }

    #[tokio::test]
    async fn test_mod_runner_flash_map_and_dtc_mask() {
        // Positive path: scanned provenance, preconditions that match the mock.
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![
                patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3])),
                dtc_mask(0x00, MapProvenance::Scanned),
            ],
            vec![],
            12.5,
        );
        let exec = ModRunner::apply_mod(
            &mut iface,
            &mut m,
            &vin(7),
            12.8,
            TargetFingerprintPolicy::Enforce,
        )
        .await
        .unwrap();
        assert!(exec.success);
        assert_eq!(exec.steps_completed, 2);
        assert_eq!(exec.total_steps, 2);
        assert!(exec.actions_executed[0].contains("Torque Limiter"));
        assert!(exec.actions_executed[1].contains("P0401"));
        assert!(exec.git_commit_sha.is_some());
    }

    #[tokio::test]
    async fn test_hw_whitelist_read_failure_refuses() {
        let mut iface = VirtualCanInterface::with_failing_services(&[0x22]);
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![write_did(None)], vec!["0281012224".into()], 12.0);
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(8),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("hardware ID could not be read"), "{msg}");

        let report = ModRunner::inspect_compatibility(&mut iface, &m, Some(&vin(8)), Some(12.8))
            .await
            .unwrap();
        assert!(!report.matched_vehicle);
    }

    /// `inspect_compatibility` must not report a package as compatible that
    /// `apply_mod` refuses outright: the three gates below live in `ModRunner`,
    /// not in `SterngateMod::check_compatibility`.
    #[tokio::test]
    async fn test_inspect_mirrors_provenance_refusal() {
        let mut iface = VirtualCanInterface::new();
        let m = runner_mod(
            vec![patch(0x1C1000, MapProvenance::Synthetic, Some(vec![0; 3]))],
            vec![],
            12.5,
        );
        let report = ModRunner::inspect_compatibility(&mut iface, &m, Some(&vin(20)), Some(12.8))
            .await
            .unwrap();
        assert!(report.is_valid, "the package itself is intact");
        assert!(!report.matched_vehicle);
        assert!(
            report
                .warning_messages
                .iter()
                .any(|w| w.contains("provenance")),
            "{:?}",
            report.warning_messages
        );
    }

    #[tokio::test]
    async fn test_inspect_mirrors_erase_routine_refusal() {
        let mut iface = VirtualCanInterface::new();
        let m = runner_mod(
            vec![ModAction::Routine {
                routine_id: 0xFF00,
                subfunction: 0x01,
                data: vec![],
                description: "erase".into(),
            }],
            vec![],
            12.0,
        );
        let report = ModRunner::inspect_compatibility(&mut iface, &m, Some(&vin(21)), Some(12.8))
            .await
            .unwrap();
        assert!(!report.matched_vehicle);
        assert!(
            report
                .warning_messages
                .iter()
                .any(|w| w.contains("EraseMemory")),
            "{:?}",
            report.warning_messages
        );
    }

    #[tokio::test]
    async fn test_inspect_mirrors_flash_voltage_floor() {
        // `SterngateMod::create` refuses a flash package declaring less than
        // 12.5 V, so build a compliant package and swap the action in the way a
        // deserialized (never-`create`d) package could arrive: the declared
        // 12.0 V floor now lets `check_compatibility` alone wave 12.2 V through.
        let mut m = runner_mod(vec![write_did(None)], vec![], 12.0);
        m.actions = vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3]))];
        resign(&mut m);

        let mut iface = VirtualCanInterface::new();
        let report = ModRunner::inspect_compatibility(&mut iface, &m, Some(&vin(22)), Some(12.2))
            .await
            .unwrap();
        assert!(report.is_valid, "{:?}", report.warning_messages);
        assert!(!report.matched_vehicle);
        assert!(
            report.warning_messages.iter().any(|w| w.contains("12.5")),
            "{:?}",
            report.warning_messages
        );

        // Above the floor the same package inspects clean.
        let report = ModRunner::inspect_compatibility(&mut iface, &m, Some(&vin(22)), Some(12.8))
            .await
            .unwrap();
        assert!(report.matched_vehicle, "{:?}", report.warning_messages);
    }

    #[tokio::test]
    async fn test_bitmask_write_read_failure_refuses() {
        let mut iface = VirtualCanInterface::with_failing_services(&[0x22]);
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![write_did(Some(vec![0x02]))], vec![], 12.0);
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(9),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("bitmask"), "{msg}");

        // Positive control on a healthy mock.
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(vec![write_did(Some(vec![0x02]))], vec![], 12.0);
        assert!(ModRunner::apply_mod(
            &mut iface,
            &mut m,
            &vin(9),
            12.8,
            TargetFingerprintPolicy::Enforce
        )
        .await
        .is_ok());
    }

    #[tokio::test]
    async fn test_overlapping_flash_ranges_refused() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![
                patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3])),
                patch(0x1C1002, MapProvenance::Scanned, Some(vec![0; 3])),
            ],
            vec![],
            12.5,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(10),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("overlap"), "{msg}");
    }

    #[tokio::test]
    async fn test_bypass_policy_never_bypasses_voltage_or_flash_packages() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();

        // Voltage floor is enforced under BypassUnsafe.
        let mut m = runner_mod(vec![write_did(None)], vec![], 12.0);
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(11),
                11.0,
                TargetFingerprintPolicy::BypassUnsafe,
            )
            .await,
        );
        assert!(msg.contains("voltage"), "{msg}");

        // BypassUnsafe is refused outright for packages that write flash.
        let mut m = runner_mod(
            vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3]))],
            vec![],
            12.5,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(11),
                12.8,
                TargetFingerprintPolicy::BypassUnsafe,
            )
            .await,
        );
        assert!(msg.contains("not permitted"), "{msg}");

        // BypassUnsafe still relaxes the chassis check for DID writes.
        let mut m = runner_mod(vec![write_did(None)], vec![], 12.0);
        assert!(ModRunner::apply_mod(
            &mut iface,
            &mut m,
            "WDB2040011A999999",
            12.8,
            TargetFingerprintPolicy::BypassUnsafe
        )
        .await
        .is_ok());
    }

    #[tokio::test]
    async fn test_flash_write_floor_overrides_author_declared_minimum() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        // create() refuses < 12.5 V for flash writes, so forge the target after
        // creation; integrity v2 covers the target, so the forged package must
        // be re-signed the way an attacker would to pass the integrity check.
        let mut m = runner_mod(
            vec![patch(0x1C1000, MapProvenance::Scanned, Some(vec![0; 3]))],
            vec![],
            12.5,
        );
        m.target.min_battery_voltage = 12.0;
        resign(&mut m);
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(12),
                12.2,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("12.5"), "{msg}");
    }

    #[tokio::test]
    async fn test_erase_memory_routine_refused() {
        // Interface deliberately NOT opened: the gate must fire before any bus traffic.
        let mut iface = VirtualCanInterface::new();
        let mut m = runner_mod(
            vec![ModAction::Routine {
                routine_id: 0xFF00,
                subfunction: 0x01,
                data: vec![],
                description: "erase".into(),
            }],
            vec![],
            12.0,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(13),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("EraseMemory"), "{msg}");

        // Positive control: other routine IDs stay allowed.
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![ModAction::Routine {
                routine_id: 0x0201,
                subfunction: 0x01,
                data: vec![],
                description: "reset injector adaptation".into(),
            }],
            vec![],
            12.0,
        );
        let exec = ModRunner::apply_mod(
            &mut iface,
            &mut m,
            &vin(13),
            12.8,
            TargetFingerprintPolicy::Enforce,
        )
        .await
        .unwrap();
        assert_eq!(exec.steps_completed, 1);
    }

    #[tokio::test]
    async fn test_bitmask_length_mismatch_refused() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![ModAction::WriteDid {
                did: 0x0201,
                data: vec![0x00, 0x00],
                bitmask: Some(vec![0x01]),
                expected_original_data: None,
                description: "seatbelt chime".into(),
            }],
            vec![],
            12.0,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(14),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("bitmask length"), "{msg}");
    }

    #[tokio::test]
    async fn test_write_did_empty_precondition_refused() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();
        let mut m = runner_mod(
            vec![ModAction::WriteDid {
                did: 0x0201,
                data: vec![0x00],
                bitmask: None,
                expected_original_data: Some(vec![]),
                description: "seatbelt chime".into(),
            }],
            vec![],
            12.0,
        );
        let msg = err_text(
            ModRunner::apply_mod(
                &mut iface,
                &mut m,
                &vin(15),
                12.8,
                TargetFingerprintPolicy::Enforce,
            )
            .await,
        );
        assert!(msg.contains("empty precondition"), "{msg}");
    }

    #[tokio::test]
    async fn test_generic_routine_and_vin_adaptation() {
        let mut iface = VirtualCanInterface::new();
        iface.open().await.unwrap();

        // 1. Test generic routine execution (e.g. 0x0305 Steering Angle Zero Position)
        let routine_res = ServiceRoutineManager::execute_generic_routine(
            &mut iface,
            0x7E0,
            0x7E8,
            0x0305,
            &[0x00],
        )
        .await
        .unwrap();
        assert!(!routine_res.is_empty());

        // 2. Test Donor ECU Re-VIN Adaptation
        let adapt_res = VinAdaptationManager::adapt_donor_ecu_vin(
            &mut iface,
            0x7E0,
            0x7E8,
            "CR4",
            "WDB2112061A999888",
            Some(0x0B),
        )
        .await
        .unwrap();

        assert!(adapt_res.success);
        assert_eq!(adapt_res.new_vin, "WDB2112061A999888");
        assert_eq!(adapt_res.security_level, "0x0B");
        assert!(adapt_res.message.contains("successful"));
    }
}
