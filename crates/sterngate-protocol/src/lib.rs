pub mod discoverer;
pub mod flasher;
pub mod gate;
pub mod isotp;
pub mod kwp2000;
pub mod scanner;
pub mod seedkey;
pub mod service;
pub mod uds;

pub use discoverer::BusDiscoverer;
pub use flasher::FlashingWorker;
pub use gate::TransactionGate;
pub use isotp::IsoTpChannel;
pub use kwp2000::KwpClient;
pub use scanner::{ModuleScanResult, VehicleDiagnosticReport, VehicleScanner};
pub use seedkey::{DaimlerSeedKey, DaimlerSolver, SeedKeySolver};
pub use service::ServiceRoutineManager;
pub use uds::UdsClient;

#[cfg(test)]
mod tests {
    use super::*;
    use sterngate_core::Language;
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
        use sterngate_core::{SuspensionCorner, SuspensionCornerAction};
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
}
