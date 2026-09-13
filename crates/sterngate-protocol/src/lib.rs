pub mod flasher;
pub mod isotp;
pub mod kwp2000;
pub mod seedkey;
pub mod uds;

pub use flasher::FlashingWorker;
pub use isotp::IsoTpChannel;
pub use kwp2000::KwpClient;
pub use seedkey::{DaimlerSeedKey, DaimlerSolver, SeedKeySolver};
pub use uds::UdsClient;

#[cfg(test)]
mod tests {
    use super::*;
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
}
