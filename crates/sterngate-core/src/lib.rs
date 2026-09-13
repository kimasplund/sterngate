pub mod analytics;
pub mod catalog;
pub mod command;
pub mod dtc;
pub mod error;
pub mod flash;
pub mod frame;
pub mod garage;
pub mod i18n;
pub mod parameter;
pub mod profile;

pub use analytics::{
    CompressorGuardAction, CompressorOperationalState, CompressorProtectionGuard, DriveBenchmark,
    DriveComparison, DriveSample, DriveSummary, SuspensionHealthReport, SuspensionLeakDetector,
    SuspensionSample, SuspensionStatus,
};
pub use catalog::{
    CatalogMetadata, CbfCatalog, CbfEcuEntry, CbfVersionInfo, EcuCatalog, EcuCatalogEntry,
    EcuSearchResult, EcuVersionInfo,
};
pub use command::{CommandEnvelope, CommandValidationReport};
pub use dtc::Dtc;
pub use error::{Result, SterngateError};
pub use flash::{FlashPackageManifest, FlashProgress, FlashState, PreFlightReport};
pub use frame::CanFrame;
pub use garage::{DecodedVin, GitCommitInfo, VehicleEcuSnapshot, VehicleGarage, VehicleRecord};
pub use i18n::{lookup_dtc_description, lookup_routine_name, Language};
pub use parameter::{ParameterValue, TelemetrySnapshot};
pub use profile::{ModuleDef, ParameterDef, ScalingDef, VehicleProfile};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_frame_creation() {
        let frame = CanFrame::new_standard(0x7E0, &[0x02, 0x10, 0x01]);
        assert_eq!(frame.id, 0x7E0);
        assert!(!frame.is_extended);
        assert_eq!(frame.dlc, 3);
        assert_eq!(frame.data, vec![0x02, 0x10, 0x01]);
    }

    #[test]
    fn test_dtc_parsing() {
        let dtc = Dtc::parse_iso15031(0x01, 0x00, 0x28, "EDC16");
        assert_eq!(dtc.code, "P0100");
        assert!(dtc.confirmed);
        assert!(!dtc.pending);
        assert_eq!(
            dtc.description,
            "Mass Air Flow (MAF) Sensor Circuit Malfunction"
        );
    }

    #[test]
    fn test_parameter_scaling() {
        let param = ParameterDef {
            id: "trans_temp".into(),
            name: "Transmission Fluid Temp".into(),
            names: std::collections::HashMap::new(),
            module: "EGS52".into(),
            service: 0x22,
            did: "0x2001".into(),
            byte_offset: 0,
            length: 1,
            scaling: ScalingDef {
                slope: 1.0,
                offset: -40.0,
            },
            unit: "°C".into(),
            min: Some(-40.0),
            max: Some(150.0),
        };

        // 120 (0x78) - 40 = 80°C
        let res = param.parse_raw(&[0x62, 0x20, 0x01, 120]).unwrap();
        assert_eq!(res.value, 80.0);
        assert_eq!(res.unit, "°C");
    }

    #[test]
    fn test_load_sample_profile() {
        let profile =
            VehicleProfile::load_from_file("../../profiles/mercedes/w211_om646_edc16.json")
                .unwrap();
        assert_eq!(profile.profile_name, "mercedes_w211_om646_edc16");
        assert_eq!(profile.oem, "Mercedes-Benz");
        assert!(profile.modules.contains_key("EDC16"));
        assert!(profile.modules.contains_key("EGS52"));
        let edc16 = profile.get_module("EDC16").unwrap();
        assert_eq!(edc16.tx_can_id().unwrap(), 0x7E0);
        assert_eq!(edc16.rx_can_id().unwrap(), 0x7E8);
    }

    #[test]
    fn test_command_envelope_integrity_validation() {
        let payload = vec![0x12, 0x34, 0x56, 0x78];
        let env = CommandEnvelope::new("EDC16", 0x2E, Some(0x2001), payload.clone());

        // Valid envelope must pass
        assert!(env.validate_integrity().is_ok());

        // Truncated envelope (e.g. lost 1 byte over Wi-Fi drop)
        let mut truncated = env.clone();
        truncated.payload.pop();
        assert!(truncated.validate_integrity().is_err());

        // Bit-flipped envelope (checksum mismatch)
        let mut corrupted = env.clone();
        corrupted.payload[0] ^= 0xFF;
        assert!(corrupted.validate_integrity().is_err());
    }

    #[test]
    fn test_command_envelope_ttl_expiration() {
        let env = CommandEnvelope::new("EDC16", 0x14, None, vec![]).with_ttl(1000);
        let current_time = env.timestamp_ms + 500;
        assert!(!env.is_expired(current_time));

        let expired_time = env.timestamp_ms + 1500;
        assert!(env.is_expired(expired_time));
    }

    #[test]
    fn test_ecu_catalog_loading_and_search() {
        let catalog = EcuCatalog::load_default().unwrap();
        let stats = catalog.stats();
        assert_eq!(stats.unique_ecus, 990);

        // Search for EGS
        let egs_results = catalog.search("EGS", 10);
        assert!(!egs_results.is_empty());
        assert!(egs_results.iter().any(|r| r.ecu_name == "EGS52"));

        // Exact get_ecu inspection
        let egs52 = catalog.get_ecu("EGS52").unwrap();
        assert_eq!(egs52.ecu_name, "EGS52");
        assert_eq!(egs52.protocol, "UDS");
        assert_eq!(egs52.tx_id.as_deref(), Some("0x7e1"));
        assert_eq!(egs52.rx_id.as_deref(), Some("0x7e9"));
        assert_eq!(egs52.func_id.as_deref(), Some("0x7df"));
        assert_eq!(egs52.dtc_count, 114);
        assert!(!egs52.chassis.is_empty());

        // VGSNAG2 check
        let vgs = catalog.get_ecu("VGSNAG2").unwrap();
        assert_eq!(vgs.protocol, "UDS");
        assert_eq!(vgs.dtc_count, 258);
        assert!(!vgs.chassis.is_empty());
    }

    #[test]
    fn test_profile_discovery() {
        let profiles = VehicleProfile::discover("../../profiles");
        assert!(!profiles.is_empty());
        assert!(profiles
            .iter()
            .any(|p| p.profile_name == "mercedes_w211_om646_edc16"));
    }

    #[test]
    fn test_multilingual_dtc_and_routine_lookups() {
        // English
        let dtc_en = Dtc::parse_iso15031(0x01, 0x00, 0x28, "EDC16");
        assert_eq!(
            dtc_en.description,
            "Mass Air Flow (MAF) Sensor Circuit Malfunction"
        );
        let routine_en = lookup_routine_name(0xFF01, Language::En);
        assert_eq!(routine_en, "Fuel Pump Prime & Rail Bleed");

        // German
        let dtc_de = Dtc::parse_iso15031_localized(0x01, 0x00, 0x28, "EDC16", Language::De);
        assert_eq!(
            dtc_de.description,
            "Luftmassenmesser (LMM) Schaltkreis Fehlfunktion"
        );
        let routine_de = lookup_routine_name(0xFF01, Language::De);
        assert_eq!(routine_de, "Kraftstoffpumpe vorfördern & Entlüftung");

        // Swedish
        let dtc_sv = Dtc::parse_iso15031_localized(0x01, 0x00, 0x28, "EDC16", Language::Sv);
        assert_eq!(dtc_sv.description, "Luftmassemätare (LMM) Strömkretsfel");
        let routine_sv = lookup_routine_name(0xFF01, Language::Sv);
        assert_eq!(routine_sv, "Bränslepump grundning och urluftning");
    }

    #[test]
    fn test_profile_multilingual_parameters_and_modules() {
        let mut prof =
            VehicleProfile::load_from_file("../../profiles/mercedes/w203_om646_cr3.json").unwrap();

        let tcc = prof.find_parameter("tcc_slip_rpm").unwrap();
        assert_eq!(tcc.name, "Torque Converter Clutch Slip");
        assert_eq!(tcc.localized_name(Language::De), "Drehzahldifferenz KÜB");
        assert_eq!(
            tcc.localized_name(Language::Sv),
            "Momentomvandlarkoppling slirning"
        );

        let egs = prof.get_module("EGS52").unwrap();
        assert_eq!(
            egs.localized_name(Language::De),
            "Elektronische Getriebesteuerung (722.6 / NAG1)"
        );

        // In-place localization test
        prof.localize(Language::De);
        let tcc_localized = prof.find_parameter("tcc_slip_rpm").unwrap();
        assert_eq!(tcc_localized.name, "Drehzahldifferenz KÜB");

        let egs_localized = prof.get_module("EGS52").unwrap();
        assert_eq!(
            egs_localized.name,
            "Elektronische Getriebesteuerung (722.6 / NAG1)"
        );
    }

    #[test]
    fn test_vin_decoding_s211() {
        // User's OM646 S211 Estate (E 220 T CDI)
        let s211_vin = DecodedVin::decode("WDB2112061A892341");
        assert_eq!(s211_vin.manufacturer, "Mercedes-Benz");
        assert_eq!(s211_vin.body_style, "Estate / T-Modell (S211)");
        assert_eq!(s211_vin.model_name, "E 220 T CDI");
        assert!(s211_vin.engine.contains("OM646"));

        // W211 Sedan (E 320 CDI V6)
        let w211_vin = DecodedVin::decode("WDB2110221A123456");
        assert_eq!(w211_vin.body_style, "Sedan / Saloon (W211)");
        assert_eq!(w211_vin.model_name, "E 320 CDI V6");
    }

    #[test]
    fn test_suspension_leak_detection() {
        let mut detector = SuspensionLeakDetector::new();
        // Simulate stationary drop over 1 hour
        detector.add_sample(SuspensionSample {
            timestamp_ms: 1000,
            left_rear_height_mm: 120.0,
            right_rear_height_mm: 120.0,
            compressor_active: false,
            compressor_run_duration_s: 0.0,
            ..Default::default()
        });
        // 1 hour later: left dropped by 12mm (severe leak), compressor had to run 65s
        detector.add_sample(SuspensionSample {
            timestamp_ms: 1000 + 3_600_000,
            left_rear_height_mm: 108.0,
            right_rear_height_mm: 119.5,
            compressor_active: true,
            compressor_run_duration_s: 65.0,
            ..Default::default()
        });

        let report = detector.evaluate();
        assert_eq!(report.status, SuspensionStatus::CriticalLeak);
        assert!(report.height_drop_rate_mm_per_hour >= 10.0);
        assert!(report.max_compressor_continuous_run_s > 60.0);
        assert!(!report.recommendations.is_empty());
    }

    #[test]
    fn test_drive_benchmark_and_comparison() {
        let mut b_run1 = DriveBenchmark::new();
        // Run 1: High slip, 7.8 L/100km
        b_run1.add_sample(DriveSample {
            timestamp_ms: 1000,
            speed_kmh: 100.0,
            engine_rpm: 2000.0,
            injection_mass_mg_str: 35.0,
            boost_pressure_hpa: 1400.0,
            rail_pressure_bar: 1100.0,
            coolant_temp_c: 75.0,
            tcc_slip_rpm: 45.0,
            gear: 5,
        });
        b_run1.add_sample(DriveSample {
            timestamp_ms: 61000,
            speed_kmh: 100.0,
            engine_rpm: 2000.0,
            injection_mass_mg_str: 35.0,
            boost_pressure_hpa: 1400.0,
            rail_pressure_bar: 1100.0,
            coolant_temp_c: 82.0,
            tcc_slip_rpm: 42.0,
            gear: 5,
        });
        let sum1 = b_run1.summarize();
        assert!(sum1.average_consumption_l_per_100km > 0.0);

        let mut b_run2 = DriveBenchmark::new();
        // Run 2: After TCC solenoid & thermostat change -> lower slip, 7.1 L/100km, 88°C
        b_run2.add_sample(DriveSample {
            timestamp_ms: 1000,
            speed_kmh: 100.0,
            engine_rpm: 1950.0,
            injection_mass_mg_str: 31.0,
            boost_pressure_hpa: 1400.0,
            rail_pressure_bar: 1100.0,
            coolant_temp_c: 88.0,
            tcc_slip_rpm: 8.0,
            gear: 5,
        });
        b_run2.add_sample(DriveSample {
            timestamp_ms: 61000,
            speed_kmh: 100.0,
            engine_rpm: 1950.0,
            injection_mass_mg_str: 31.0,
            boost_pressure_hpa: 1400.0,
            rail_pressure_bar: 1100.0,
            coolant_temp_c: 88.0,
            tcc_slip_rpm: 7.0,
            gear: 5,
        });
        let sum2 = b_run2.summarize();

        let cmp = DriveBenchmark::compare(&sum1, &sum2, "Baseline", "New Solenoid");
        assert!(cmp.consumption_delta_l_per_100km < 0.0);
        assert!(cmp.tcc_slip_delta_rpm < 0.0);
        assert!(cmp.verdict.contains("Beneficial"));
    }

    #[test]
    fn test_vehicle_garage_lifecycle() {
        let temp_dir =
            std::env::temp_dir().join(format!("sterngate_test_garage_{}", std::process::id()));
        let garage = VehicleGarage::new(&temp_dir);

        let vin_str = "WDB2112061A999888";
        let rec = VehicleRecord {
            vin: vin_str.to_string(),
            decoded: DecodedVin::decode(vin_str),
            first_scanned: "2026-09-13T22:00:00Z".into(),
            last_scanned: "2026-09-13T22:00:00Z".into(),
            scan_count: 1,
            odometer_km: Some(250100),
            battery_voltage: Some(12.6),
            detected_modules: Default::default(),
            notes: vec!["Initial diagnostic scan".into()],
        };

        let path = garage.save_vehicle(&rec, Some("Initial scan")).unwrap();
        assert!(path.exists());

        let loaded = garage.load_vehicle(vin_str).unwrap().unwrap();
        assert_eq!(loaded.vin, vin_str);
        assert_eq!(loaded.decoded.model_name, "E 220 T CDI");

        let list = garage.list_vehicles().unwrap();
        assert!(list.iter().any(|v| v.vin == vin_str));

        // Test saving coding and git commit
        garage
            .save_coding(
                vin_str,
                "EDC16",
                "0102030405",
                None,
                "Baseline EDC16 coding",
            )
            .unwrap();

        let history = garage.get_history(vin_str).unwrap();
        assert!(!history.is_empty());

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_compressor_protection_guard_lifecycle() {
        let mut guard = CompressorProtectionGuard::new(40.0, 180.0);
        assert_eq!(guard.current_state, CompressorOperationalState::Idle);

        // 1. Normal short run (15s)
        let a1 = guard.update(1000, true);
        assert_eq!(a1, CompressorGuardAction::None);
        let a2 = guard.update(16000, true);
        assert_eq!(a2, CompressorGuardAction::None);
        let a3 = guard.update(17000, false);
        assert_eq!(a3, CompressorGuardAction::None);
        assert_eq!(guard.current_state, CompressorOperationalState::Idle);

        // 2. Overheat / continuous run > 40s -> thermal watchdog cutoff
        let _ = guard.update(20000, true);
        let action = guard.update(60500, true); // 40.5s continuous!
        match action {
            CompressorGuardAction::TripCutoff { run_duration_s, .. } => {
                assert!(run_duration_s >= 40.0);
            }
            _ => panic!("Expected thermal cutoff trip"),
        }
        match guard.current_state {
            CompressorOperationalState::ThermalCutoffTriggered {
                cooldown_remaining_seconds,
                ..
            } => {
                assert!(cooldown_remaining_seconds > 0.0);
            }
            _ => panic!("Expected ThermalCutoffTriggered state"),
        }

        // 3. Manual inhibit (Safe mode / transport mode)
        let action = guard.manual_inhibit("User transport mode");
        match action {
            CompressorGuardAction::TripCutoff { reason, .. } => {
                assert!(reason.contains("User transport mode"));
            }
            _ => panic!("Expected trip cutoff for manual inhibit"),
        }
        assert!(guard.is_inhibited);

        // 4. Restore normal operation
        let restore = guard.manual_restore();
        assert_eq!(restore, CompressorGuardAction::RestoreAllowed);
        assert_eq!(guard.current_state, CompressorOperationalState::Idle);
    }
}
