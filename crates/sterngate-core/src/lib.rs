pub mod catalog;
pub mod command;
pub mod dtc;
pub mod error;
pub mod flash;
pub mod frame;
pub mod i18n;
pub mod parameter;
pub mod profile;

pub use catalog::{
    CatalogMetadata, CbfCatalog, CbfEcuEntry, CbfVersionHistoryEntry, CbfVersionInfo,
    EcuSearchResult,
};
pub use command::{CommandEnvelope, CommandValidationReport};
pub use dtc::Dtc;
pub use error::{Result, SterngateError};
pub use flash::{FlashPackageManifest, FlashProgress, FlashState, PreFlightReport};
pub use frame::CanFrame;
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
    fn test_cbf_catalog_loading_and_search() {
        let catalog = CbfCatalog::load_default().unwrap();
        let stats = catalog.stats();
        assert_eq!(stats.total_cbf_files, 2055);
        assert_eq!(stats.unique_ecus, 990);
        assert_eq!(stats.redundant_file_copies, 846);

        // Search for EGS
        let egs_results = catalog.search("EGS", 10);
        assert!(!egs_results.is_empty());
        assert!(egs_results.iter().any(|r| r.ecu_name == "EGS52"));

        // Exact get_ecu inspection
        let egs52 = catalog.get_ecu("EGS52").unwrap();
        assert_eq!(egs52.ecu_name, "EGS52");
        assert_eq!(egs52.total_copies_in_cbf, 9);
        assert_eq!(egs52.distinct_versions_count, 1);
        assert_eq!(egs52.canonical_version.protocol, "UDS");
        assert_eq!(egs52.canonical_version.tx_id.as_deref(), Some("0x7e1"));
        assert_eq!(egs52.canonical_version.rx_id.as_deref(), Some("0x7e9"));

        // VGSNAG2 multi-version check
        let vgs = catalog.get_ecu("VGSNAG2").unwrap();
        assert_eq!(vgs.total_copies_in_cbf, 10);
        assert_eq!(vgs.distinct_versions_count, 3);
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
}
