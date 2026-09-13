pub mod command;
pub mod dtc;
pub mod error;
pub mod flash;
pub mod frame;
pub mod parameter;
pub mod profile;

pub use command::{CommandEnvelope, CommandValidationReport};
pub use dtc::Dtc;
pub use error::{Result, SterngateError};
pub use flash::{FlashPackageManifest, FlashProgress, FlashState, PreFlightReport};
pub use frame::CanFrame;
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
}
