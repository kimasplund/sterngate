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
        _ => Err(format!("Resource not found: {}", uri)),
    }
}
