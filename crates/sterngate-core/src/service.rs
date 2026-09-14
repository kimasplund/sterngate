use serde::{Deserialize, Serialize};

/// SBC (Sensotronic Brake Control) Service Mode Action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SbcServiceAction {
    /// Deactivate SBC system (dump 160 bar accumulator to 0 bar, retract pistons, lock wake-up)
    Deactivate,
    /// Reactivate SBC system (recharge accumulator to ~160 bar, automated bleed check)
    Reactivate,
    /// Hydraulic pressure bleeding procedure
    Bleed,
}

/// SBC Service Mode Execution Status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SbcServiceStatus {
    pub action: SbcServiceAction,
    pub success: bool,
    pub accumulator_pressure_bar: f64,
    pub wake_up_suppressed: bool,
    pub service_mode_active: bool,
    pub message: String,
}

/// Common Rail Injector IMA (Injector Quantity Adaptation) Classification Data
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImaClassification {
    /// Cylinder number (1-based, e.g. 1 to 4 for OM646, 1 to 6 for OM642)
    pub cylinder: u8,
    /// Alphanumeric calibration code (e.g. "7B8HNA", "A8B12F")
    pub code: String,
    /// Whether the code passes length and character validation
    pub is_valid: bool,
    /// Injector manufacturer / format classification (e.g. "Bosch IMA (6-char)", "Delphi/Bosch EMA (7-char)")
    pub format: String,
}

impl ImaClassification {
    /// Validate and create an IMA classification entry
    pub fn new(cylinder: u8, code: impl Into<String>) -> Self {
        let code = code.into().trim().to_uppercase();
        let len = code.len();
        let is_alphanumeric = code.chars().all(|c| c.is_ascii_alphanumeric());

        let (is_valid, format) = if (len == 6 || len == 7) && is_alphanumeric {
            let fmt = if len == 6 {
                "Bosch IMA (6-character)".to_string()
            } else {
                "Bosch/Delphi EMA (7-character)".to_string()
            };
            (true, fmt)
        } else {
            (
                false,
                "Invalid Format (must be 6 or 7 alphanumeric characters)".to_string(),
            )
        };

        Self {
            cylinder,
            code,
            is_valid,
            format,
        }
    }
}

/// Air Suspension (ENR / AIRMATIC) Corner Selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuspensionCorner {
    FrontLeft,
    FrontRight,
    RearLeft,
    RearRight,
    BothRear,
    AllCorners,
}

impl SuspensionCorner {
    pub fn as_str(&self) -> &'static str {
        match self {
            SuspensionCorner::FrontLeft => "Front-Left",
            SuspensionCorner::FrontRight => "Front-Right",
            SuspensionCorner::RearLeft => "Rear-Left",
            SuspensionCorner::RearRight => "Rear-Right",
            SuspensionCorner::BothRear => "Both Rear Axle",
            SuspensionCorner::AllCorners => "All 4 Corners",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        let lower = s.to_lowercase().replace('_', "-");
        match lower.as_str() {
            "fl" | "front-left" | "frontleft" => Some(SuspensionCorner::FrontLeft),
            "fr" | "front-right" | "frontright" => Some(SuspensionCorner::FrontRight),
            "rl" | "rear-left" | "rearleft" => Some(SuspensionCorner::RearLeft),
            "rr" | "rear-right" | "rearright" => Some(SuspensionCorner::RearRight),
            "rear" | "both-rear" | "rear-axle" => Some(SuspensionCorner::BothRear),
            "all" | "all-corners" => Some(SuspensionCorner::AllCorners),
            _ => None,
        }
    }
}

/// Air Suspension Corner Actuation Action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuspensionCornerAction {
    /// Inflate air spring corner
    Inflate,
    /// Deflate air spring corner
    Deflate,
    /// Store zero-height driving calibration level
    CalibrateZeroHeight,
}

/// Discovered ECU during bus-wide interrogation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredEcu {
    /// Physical CAN arbitration request ID (e.g. 0x7E0)
    pub tx_id: u32,
    /// Physical CAN arbitration response ID (e.g. 0x7E8)
    pub rx_id: u32,
    /// Diagnostic protocol (e.g. "ISO-14229 (UDS)", "ISO-14230 (KWP2000)")
    pub protocol: String,
    /// Mercedes/OEM spare part number (DID 0xF187)
    pub part_number: Option<String>,
    /// Hardware version / number (DID 0xF191)
    pub hardware_version: Option<String>,
    /// Software version (DID 0xF189)
    pub software_version: Option<String>,
    /// Software calibration / application number (DID 0xF188)
    pub software_calibration: Option<String>,
    /// Vehicle VIN stored in module (DID 0xF190)
    pub vin: Option<String>,
    /// ECU System name (DID 0xF197 or deduced)
    pub system_name: Option<String>,
    /// Matched ECU catalog definition (from Sterngate catalog database)
    pub matched_catalog_name: Option<String>,
}
