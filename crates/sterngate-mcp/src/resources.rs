use serde_json::{json, Value};

pub fn get_resources_list() -> Value {
    json!([
        {
            "uri": "sterngate://profile/w211_om646",
            "name": "Mercedes W211 OM646 Diagnostic Profile",
            "description": "Full DID mappings, scaling equations, and module definitions for W211 CDI.",
            "mimeType": "application/json"
        },
        {
            "uri": "sterngate://ecu/status",
            "name": "Active Gateway & Bus Connection Status",
            "description": "Current interface type, baudrate, battery voltage, and gateway status.",
            "mimeType": "application/json"
        },
        {
            "uri": "sterngate://locales",
            "name": "Supported Diagnostic & UI Languages",
            "description": "Available localization languages: en (English), de (Daimler OEM German), sv (Swedish).",
            "mimeType": "application/json"
        },
        {
            "uri": "sterngate://ecu/catalog",
            "name": "Automotive ECU Catalog Statistics",
            "description": "Indexing and protocol metrics for 990 unique automotive ECUs across multi-chassis architectures.",
            "mimeType": "application/json"
        },
        {
            "uri": "sterngate://garage/vehicles",
            "name": "Vehicle Garage Database",
            "description": "List of tracked vehicles with decoded VINs, installed ECUs, and Git configuration history.",
            "mimeType": "application/json"
        },
        {
            "uri": "sterngate://cascades/catalog",
            "name": "Mercedes Cascades of Death Catalog",
            "description": "Catalog of 7 infamous Mercedes cascading failure chains, root triggers, part numbers, and failure mechanisms.",
            "mimeType": "application/json"
        }
    ])
}

pub fn read_resource(uri: &str) -> Result<Value, String> {
    match uri {
        "sterngate://profile/w211_om646" => Ok(json!({
            "profile": "mercedes_w211_om646_edc16",
            "modules": ["EDC16", "EGS52", "CGW", "AIRMATIC"],
            "key_dids": {
                "0x2001": "Transmission Fluid Temp",
                "0x2002": "TCC Lockup Slip",
                "0x200B": "Common Rail Pressure",
                "0x2021": "Cylinder 1 Injector Correction"
            }
        })),
        "sterngate://ecu/status" => Ok(json!({
            "interface": "virtual_w211_sim",
            "connected": true,
            "battery_voltage": 13.8,
            "gateway": "CGW N93",
            "baudrate": 500000,
            "flasher_locked": false
        })),
        "sterngate://locales" => Ok(json!({
            "locales": [
                { "code": "en", "name": "English", "default": true },
                { "code": "de", "name": "Deutsch (Daimler OEM)", "default": false },
                { "code": "sv", "name": "Svenska", "default": false }
            ]
        })),
        "sterngate://ecu/catalog" | "sterngate://cbf/stats" => {
            if let Ok(catalog) = sterngate_core::EcuCatalog::load_default() {
                Ok(serde_json::to_value(catalog.stats()).unwrap())
            } else {
                Ok(json!({
                    "total_ecus": 990,
                    "unique_ecus": 990
                }))
            }
        }
        "sterngate://garage/vehicles" => {
            let garage = sterngate_core::VehicleGarage::default();
            match garage.list_vehicles() {
                Ok(vehicles) => Ok(json!({
                    "total_vehicles": vehicles.len(),
                    "vehicles": vehicles
                })),
                Err(e) => Err(format!("Failed to list vehicles from garage: {}", e)),
            }
        }
        "sterngate://cascades/catalog" => Ok(json!({
            "total_cascades": 7,
            "cascades": [
                {
                    "id": "sbc_accumulator_exhaustion",
                    "name": "SBC Hydraulic Accumulator Exhaustion",
                    "root_part": "Nitrogen pressure accumulator A 000 430 26 94 (~$120)",
                    "catastrophic_outcome": "Pump motor burnout -> Total loss of power brake boost ($2,500)",
                    "threshold": "Accumulator pre-charge <70 bar (warning), <55 bar (imminent)"
                },
                {
                    "id": "common_rail_black_death",
                    "name": "Common Rail Injector 'Black Death' Blow-By",
                    "root_part": "Copper injector seal crush washer A 611 017 00 60 ($1.50)",
                    "catastrophic_outcome": "Rock-hard carbon solidifies over harness and head ($1,800–$3,500)",
                    "threshold": "Cylinder smooth running trim > +3.5 mm³/hub"
                },
                {
                    "id": "transmission_pilot_bushing_wicking",
                    "name": "722.6 Transmission Pilot Bushing ATF Capillary Wicking",
                    "root_part": "13-pin electro-hydraulic adapter bushing A 203 540 02 53 ($8)",
                    "catastrophic_outcome": "ATF wicks into EGS52 TCU, shorting MOSFETs and locking in 2nd gear ($1,500)",
                    "threshold": "ATF temp jump >20°C in <5s with speed sensor jitter"
                },
                {
                    "id": "tcc_lockup_slip",
                    "name": "722.6 Torque Converter Lockup Clutch Shredding",
                    "root_part": "TCC PWM lockup solenoid Y3/6y6 A 240 270 17 00 ($65)",
                    "catastrophic_outcome": "Clutch friction lining sheds into planetary gears and valve body ($2,800)",
                    "threshold": "TCC slip >30 RPM during commanded lockup"
                },
                {
                    "id": "dpf_differential_drift_m55",
                    "name": "DPF Differential Drift -> Turbo Bearing & M55 Swirl Flap Short",
                    "root_part": "DPF differential pressure sensor B28/8 A 006 153 95 28 ($45)",
                    "catastrophic_outcome": "Backpressure blows oil past turbo into M55 motor, blowing Fuse 54 ($3,200)",
                    "threshold": "Flat pressure <15 mbar at >3000 RPM or regen interval >1000 km"
                },
                {
                    "id": "camshaft_magnet_oil_wicking",
                    "name": "Camshaft Magnet Capillary Oil Intrusion into Engine ECU",
                    "root_part": "Cam adjustment magnets A 272 051 01 77 ($30) & pigtails A 271 150 27 33 ($15)",
                    "catastrophic_outcome": "Oil wicks inside harness into Bosch ME9.7 ECU motherboard ($2,400)",
                    "threshold": "5V reference dip with simultaneous O2 heater drift"
                },
                {
                    "id": "suspension_compressor_burnout",
                    "name": "S211 Rear Air Suspension Compressor Burnout & Welded Relay",
                    "root_part": "Rear air spring bellow A 211 320 09 25 ($140) & relay A 002 542 72 19 ($12)",
                    "catastrophic_outcome": "Continuous running melts PTFE ring; >30A current welds relay contacts closed ($1,200)",
                    "threshold": "Continuous runtime >40s, drop rate >4 mm/h, duty cycle >25%"
                }
            ]
        })),
        _ => Err(format!("Resource not found: {}", uri)),
    }
}
