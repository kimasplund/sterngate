use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dtc {
    pub code: String,
    pub raw_bytes: [u8; 3],
    pub status_byte: u8,
    pub module: String,
    pub description: String,
    pub confirmed: bool,
    pub pending: bool,
    pub warning_lamp_requested: bool,
}

impl Dtc {
    pub fn parse_iso15031(b0: u8, b1: u8, status_byte: u8, module: &str) -> Self {
        let prefix = match (b0 >> 6) & 0x03 {
            0 => 'P',
            1 => 'C',
            2 => 'B',
            3 => 'U',
            _ => 'P',
        };
        let d1 = (b0 >> 4) & 0x03;
        let d2 = b0 & 0x0F;
        let d3 = (b1 >> 4) & 0x0F;
        let d4 = b1 & 0x0F;
        let code = format!("{}{:X}{:X}{:X}{:X}", prefix, d1, d2, d3, d4);

        let confirmed = (status_byte & 0x08) != 0;
        let pending = (status_byte & 0x04) != 0;
        let warning_lamp_requested = (status_byte & 0x80) != 0;

        let description = Self::lookup_common_description(&code);

        Self {
            code,
            raw_bytes: [b0, b1, 0],
            status_byte,
            module: module.to_string(),
            description,
            confirmed,
            pending,
            warning_lamp_requested,
        }
    }

    fn lookup_common_description(code: &str) -> String {
        match code {
            "P0100" => "Mass Air Flow (MAF) Sensor Circuit Malfunction".into(),
            "P0105" => "Manifold Absolute Pressure (MAP) Sensor Circuit Malfunction".into(),
            "P0115" => "Engine Coolant Temperature Circuit Malfunction".into(),
            "P0234" => "Turbocharger/Supercharger Overboost Condition".into(),
            "P0235" => "Turbocharger Boost Sensor A Circuit Malfunction".into(),
            "P0300" => "Random/Multiple Cylinder Misfire Detected".into(),
            "P0700" => "Transmission Control System (MIL Request)".into(),
            "P0715" => "Input/Turbine Speed Sensor Circuit Malfunction".into(),
            "P0730" => "Incorrect Gear Ratio (Transmission Slip)".into(),
            "P0740" => "Torque Converter Clutch Circuit Malfunction".into(),
            "U0100" => "Lost Communication With Engine Control Module (ECM/PCM)".into(),
            "U0101" => "Lost Communication with Transmission Control Module (TCM)".into(),
            "C1500" => "Air Suspension Central Reservoir Plausibility Error".into(),
            _ => format!("Manufacturer or Standard DTC {}", code),
        }
    }
}
