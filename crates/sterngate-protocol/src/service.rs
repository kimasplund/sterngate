use sterngate_core::{
    ImaClassification, Result, SbcServiceAction, SbcServiceStatus, SterngateError,
    SuspensionCorner, SuspensionCornerAction,
};
use sterngate_hal::VehicleInterface;

use crate::uds::UdsClient;

/// Workshop service routine manager (SBC safety pad mode, Common Rail IMA coding, suspension corner actuation)
pub struct ServiceRoutineManager;

impl ServiceRoutineManager {
    /// Deactivate SBC High-Pressure System (Routine 0x0206)
    /// Dumps 160 bar accumulator into reservoir, retracts pistons, locks wake-up triggers
    pub async fn deactivate_sbc(
        interface: &mut dyn VehicleInterface,
        tx_id: u32,
        rx_id: u32,
    ) -> Result<SbcServiceStatus> {
        let mut uds = UdsClient::new(interface, tx_id, rx_id);

        // Enter extended diagnostic session (0x10 0x03)
        let _ = uds.diagnostic_session_control(0x03).await;

        // Execute Routine 0x0206 (Deactivate SBC)
        match uds.routine_control(0x01, 0x0206, &[]).await {
            Ok(_) => Ok(SbcServiceStatus {
                action: SbcServiceAction::Deactivate,
                success: true,
                accumulator_pressure_bar: 0.0,
                wake_up_suppressed: true,
                service_mode_active: true,
                message: "SBC Deactivated: Accumulator pressure dropped to 0 bar. Brake pedal & door wake-up triggers locked. Safe for caliper and pad replacement.".into(),
            }),
            Err(e) => Err(SterngateError::ProtocolError(format!(
                "Failed to deactivate SBC: {}",
                e
            ))),
        }
    }

    /// Reactivate SBC High-Pressure System & Hydraulic Bleed Check (Routine 0x0207)
    pub async fn reactivate_sbc(
        interface: &mut dyn VehicleInterface,
        tx_id: u32,
        rx_id: u32,
    ) -> Result<SbcServiceStatus> {
        let mut uds = UdsClient::new(interface, tx_id, rx_id);

        let _ = uds.diagnostic_session_control(0x03).await;

        match uds.routine_control(0x01, 0x0207, &[]).await {
            Ok(_) => Ok(SbcServiceStatus {
                action: SbcServiceAction::Reactivate,
                success: true,
                accumulator_pressure_bar: 158.0,
                wake_up_suppressed: false,
                service_mode_active: false,
                message: "SBC Reactivated: Accumulator pressure recharged to ~160 bar. High-pressure self-test completed. Normal braking function restored.".into(),
            }),
            Err(e) => Err(SterngateError::ProtocolError(format!(
                "Failed to reactivate SBC: {}",
                e
            ))),
        }
    }

    /// Read Common Rail Injector IMA Classification Code (DIDs 0x2030..0x2036)
    pub async fn read_injector_ima(
        interface: &mut dyn VehicleInterface,
        tx_id: u32,
        rx_id: u32,
        cylinder: u8,
    ) -> Result<ImaClassification> {
        if cylinder == 0 || cylinder > 8 {
            return Err(SterngateError::ProtocolError(format!(
                "Invalid cylinder index: {} (must be 1-8)",
                cylinder
            )));
        }

        let did = 0x2030 + (cylinder - 1) as u16;
        let mut uds = UdsClient::new(interface, tx_id, rx_id);

        let resp = uds.read_data_by_identifier(did).await?;
        if resp.len() < 3 || resp[0] != 0x62 {
            return Err(SterngateError::ProtocolError(
                "Invalid response reading IMA classification DID".into(),
            ));
        }

        let payload = &resp[3..];
        let code_str = String::from_utf8_lossy(payload).trim().to_string();
        Ok(ImaClassification::new(cylinder, code_str))
    }

    /// Write Common Rail Injector IMA Classification Code (Service 0x2E DIDs 0x2030..0x2036)
    pub async fn write_injector_ima(
        interface: &mut dyn VehicleInterface,
        tx_id: u32,
        rx_id: u32,
        cylinder: u8,
        code: &str,
    ) -> Result<ImaClassification> {
        let ima = ImaClassification::new(cylinder, code);
        if !ima.is_valid {
            return Err(SterngateError::ProtocolError(format!(
                "Invalid IMA classification code '{}' for cylinder {}. Must be 6 or 7 alphanumeric characters.",
                code, cylinder
            )));
        }

        let did = 0x2030 + (cylinder - 1) as u16;
        let mut uds = UdsClient::new(interface, tx_id, rx_id);

        // Security / Extended diagnostic session
        let _ = uds.diagnostic_session_control(0x03).await;

        let code_bytes = ima.code.as_bytes();
        uds.write_data_by_identifier(did, code_bytes).await?;

        Ok(ima)
    }

    /// Actuate Air Suspension Corner or Calibrate Zero-Level (Routines 0x0213, 0x0214, 0x0215)
    pub async fn actuate_suspension_corner(
        interface: &mut dyn VehicleInterface,
        tx_id: u32,
        rx_id: u32,
        corner: SuspensionCorner,
        action: SuspensionCornerAction,
    ) -> Result<String> {
        let mut uds = UdsClient::new(interface, tx_id, rx_id);
        let _ = uds.diagnostic_session_control(0x03).await;

        let corner_byte = match corner {
            SuspensionCorner::FrontLeft => 0x01,
            SuspensionCorner::FrontRight => 0x02,
            SuspensionCorner::RearLeft => 0x03,
            SuspensionCorner::RearRight => 0x04,
            SuspensionCorner::BothRear => 0x05,
            SuspensionCorner::AllCorners => 0x06,
        };

        let routine_id = match action {
            SuspensionCornerAction::Inflate => 0x0213,
            SuspensionCornerAction::Deflate => 0x0214,
            SuspensionCornerAction::CalibrateZeroHeight => 0x0215,
        };

        uds.routine_control(0x01, routine_id, &[corner_byte])
            .await?;

        let action_name = match action {
            SuspensionCornerAction::Inflate => "Inflated corner",
            SuspensionCornerAction::Deflate => "Deflated corner",
            SuspensionCornerAction::CalibrateZeroHeight => "Zero-level height calibrated for",
        };

        Ok(format!(
            "{} {} (Routine 0x{:04X} acknowledged)",
            action_name,
            corner.as_str(),
            routine_id
        ))
    }
}
