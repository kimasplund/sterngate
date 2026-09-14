use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::time::Duration;
use tokio::time::timeout;

use sterngate_core::{
    CanFrame, DiscoveredEcu, EcuCatalog, ModuleDef, ParameterDef, Result, ScalingDef,
    VehicleProfile,
};
use sterngate_hal::VehicleInterface;

use crate::uds::UdsClient;

/// Engine for bus-wide ECU interrogation, discovery, and profile generation
pub struct BusDiscoverer;

impl BusDiscoverer {
    /// Discover active ECUs across a CAN ID range (e.g. 0x700..=0x7EF)
    pub async fn discover_ecus(
        interface: &mut dyn VehicleInterface,
        id_range: RangeInclusive<u32>,
        timeout_per_id_ms: u64,
        catalog: Option<&EcuCatalog>,
    ) -> Result<Vec<DiscoveredEcu>> {
        let mut discovered = Vec::new();
        let timeout_duration = Duration::from_millis(timeout_per_id_ms.max(15));

        for tx_id in id_range {
            // Send UDS DiagnosticSessionControl (0x10 0x01: defaultSession)
            let ping_frame = CanFrame::new_standard(tx_id as u16, &[0x02, 0x10, 0x01]);
            if interface.send(ping_frame).await.is_err() {
                continue;
            }

            // Wait for response frame
            let rx_res = timeout(timeout_duration, interface.recv()).await;
            if let Ok(Ok(resp_frame)) = rx_res {
                let rx_id = resp_frame.id;
                // Check if frame looks like a diagnostic response (0x50, 0x7E, 0x7F, or length > 1)
                let is_diag_response = resp_frame.data.len() >= 2
                    && (resp_frame.data[1] == 0x50
                        || resp_frame.data[1] == 0x7E
                        || resp_frame.data[1] == 0x7F);

                if is_diag_response || resp_frame.id == (tx_id + 8) {
                    // Responsive module detected! Interrogate identification DIDs
                    let mut ecu = Self::interrogate_ecu(interface, tx_id, rx_id).await;

                    // Match against ECU catalog if available
                    if let Some(cat) = catalog {
                        Self::match_with_catalog(&mut ecu, cat);
                    }

                    discovered.push(ecu);
                }
            }
        }

        Ok(discovered)
    }

    /// Query standard identification DIDs from a responsive ECU
    async fn interrogate_ecu(
        interface: &mut dyn VehicleInterface,
        tx_id: u32,
        rx_id: u32,
    ) -> DiscoveredEcu {
        let mut uds = UdsClient::new(interface, tx_id, rx_id);

        let part_number = Self::read_did_string(&mut uds, 0xF187).await;
        let software_calibration = Self::read_did_string(&mut uds, 0xF188).await;
        let software_version = Self::read_did_string(&mut uds, 0xF189).await;
        let vin = Self::read_did_string(&mut uds, 0xF190).await;
        let hardware_version = Self::read_did_string(&mut uds, 0xF191).await;
        let system_name = Self::read_did_string(&mut uds, 0xF197).await;

        DiscoveredEcu {
            tx_id,
            rx_id,
            protocol: "ISO-14229 (UDS)".to_string(),
            part_number,
            hardware_version,
            software_version,
            software_calibration,
            vin,
            system_name,
            matched_catalog_name: None,
        }
    }

    /// Read a DID and format printable ASCII/hex string
    async fn read_did_string(uds: &mut UdsClient<'_>, did: u16) -> Option<String> {
        if let Ok(bytes) = uds.read_data_by_identifier(did).await {
            if bytes.len() >= 3 && bytes[0] == 0x62 {
                let payload = &bytes[3..];
                if payload.is_empty() {
                    return None;
                }

                // Check if predominantly ASCII characters
                let ascii_count = payload
                    .iter()
                    .filter(|&&b| b.is_ascii_graphic() || b == b' ')
                    .count();
                if ascii_count >= payload.len() / 2 {
                    let s = String::from_utf8_lossy(payload).trim().to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }

                // Fallback to hex string representation
                let hex = payload
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ");
                return Some(hex);
            }
        }
        None
    }

    /// Match discovered ECU details with the 990-ECU database
    fn match_with_catalog(ecu: &mut DiscoveredEcu, catalog: &EcuCatalog) {
        // 1. Match by Part Number
        if let Some(pn) = &ecu.part_number {
            let clean_pn = pn.replace([' ', '-', '.'], "").to_uppercase();
            for (ecu_id, entry) in &catalog.ecus {
                let entry_clean = entry.ecu_name.replace([' ', '-', '.'], "").to_uppercase();
                if !entry_clean.is_empty()
                    && (entry_clean.contains(&clean_pn) || clean_pn.contains(&entry_clean))
                {
                    ecu.matched_catalog_name =
                        Some(format!("{} (ECU: {})", entry.ecu_name, ecu_id));
                    return;
                }
            }
        }

        // 2. Match by System Name
        if let Some(sys) = &ecu.system_name {
            let query = sys.to_lowercase();
            let results = catalog.search(&query, 5);
            if let Some(best) = results.first() {
                ecu.matched_catalog_name = Some(format!("{} ({})", best.ecu_name, best.protocol));
                return;
            }
        }

        // 3. Fallback deduce by CAN request ID
        let deduced = match ecu.tx_id {
            0x7DF => "Central Gateway (Functional OBD/UDS)",
            0x7E0 => "Engine Control Unit (EDC / ME / CDI)",
            0x7E1 => "Transmission Control Unit (EGS / VGS)",
            0x7E2 => "Braking & Stability Control (ESP / SBC)",
            0x7E3 => "Central Gateway / Steering Column (CGW / SCM)",
            0x7E4 => "Air Suspension / Leveling (ENR / AIRMATIC)",
            0x7E5 => "Front Signal Acquisition Module (SAM-F)",
            0x7E6 => "Active Body Control (ABC)",
            0x7E7 => "Instrument Cluster (IC / KI)",
            _ => "Generic Automotive ECU",
        };
        ecu.matched_catalog_name = Some(deduced.to_string());
    }

    /// Automatically generate a declarative VehicleProfile from discovered ECUs
    pub fn generate_profile(
        discovered: &[DiscoveredEcu],
        oem: &str,
        chassis: &str,
        profile_name: &str,
    ) -> VehicleProfile {
        let mut modules = HashMap::new();

        for ecu in discovered {
            let key = ecu
                .system_name
                .clone()
                .or_else(|| {
                    ecu.matched_catalog_name.as_ref().map(|n| {
                        n.split_whitespace()
                            .next()
                            .unwrap_or("ECU")
                            .replace(['(', ')', '-', '/'], "")
                    })
                })
                .unwrap_or_else(|| format!("ECU_{:03X}", ecu.tx_id));

            let desc = ecu
                .matched_catalog_name
                .clone()
                .unwrap_or_else(|| format!("Discovered ECU at 0x{:03X}", ecu.tx_id));

            let mut names = HashMap::new();
            names.insert("en".to_string(), desc.clone());
            names.insert("de".to_string(), format!("Steuergerät 0x{:03X}", ecu.tx_id));
            names.insert("sv".to_string(), format!("Styrenhet 0x{:03X}", ecu.tx_id));

            modules.insert(
                key,
                ModuleDef {
                    name: desc,
                    tx_id: format!("0x{:03X}", ecu.tx_id),
                    rx_id: format!("0x{:03X}", ecu.rx_id),
                    protocol: "UDS".to_string(),
                    seed_key_algo: Some("Daimler_Level1".to_string()),
                    names,
                },
            );
        }

        // Standard default parameters
        let parameters = vec![
            ParameterDef {
                id: "engine_speed".to_string(),
                name: "Engine RPM".to_string(),
                names: HashMap::new(),
                module: "EDC16".to_string(),
                service: 0x22,
                did: "0x0100".to_string(),
                byte_offset: 0,
                length: 2,
                scaling: ScalingDef {
                    slope: 0.25,
                    offset: 0.0,
                },
                unit: "RPM".to_string(),
                min: Some(0.0),
                max: Some(6000.0),
            },
            ParameterDef {
                id: "coolant_temp".to_string(),
                name: "Coolant Temperature".to_string(),
                names: HashMap::new(),
                module: "EDC16".to_string(),
                service: 0x22,
                did: "0x0105".to_string(),
                byte_offset: 0,
                length: 1,
                scaling: ScalingDef {
                    slope: 1.0,
                    offset: -40.0,
                },
                unit: "°C".to_string(),
                min: Some(-40.0),
                max: Some(150.0),
            },
        ];

        VehicleProfile {
            profile_name: profile_name.to_string(),
            oem: oem.to_string(),
            chassis: chassis.to_string(),
            gateway_type: Some("CGW".to_string()),
            default_bitrate: 500000,
            modules,
            parameters,
        }
    }
}
