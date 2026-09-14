use anyhow::Result;
use std::path::PathBuf;
use tracing::warn;

use sterngate_core::{decode_from_armor, SterngateMod, VehicleProfile};
use sterngate_hal::{OpenPortInterface, SocketCanInterface, VehicleInterface, VirtualCanInterface};

pub fn load_profile_safe(path: &PathBuf) -> VehicleProfile {
    VehicleProfile::load_from_file(path).unwrap_or_else(|e| {
        warn!(
            "Could not load profile at {}: {}. Using default W211 configuration.",
            path.display(),
            e
        );
        VehicleProfile {
            profile_name: "fallback_w211".into(),
            oem: "Mercedes-Benz".into(),
            chassis: "W211".into(),
            gateway_type: Some("CGW_N93".into()),
            default_bitrate: 500000,
            modules: Default::default(),
            parameters: vec![],
        }
    })
}

pub async fn open_interface(can_interface: &str) -> Box<dyn VehicleInterface> {
    if can_interface == "mock" || can_interface == "sim" {
        let mut sim = VirtualCanInterface::new();
        let _ = sim.open().await;
        Box::new(sim)
    } else if can_interface == "openport" || can_interface == "tactrix" {
        let mut op = OpenPortInterface::new();
        if op.open().await.is_ok() {
            Box::new(op)
        } else {
            warn!(
                "Tactrix OpenPort hardware not found on USB, falling back to simulated interface"
            );
            let (mut sim_op, _) = OpenPortInterface::new_simulated(12.65);
            let _ = sim_op.open().await;
            Box::new(sim_op)
        }
    } else {
        let mut can = SocketCanInterface::new(can_interface);
        if can.open().await.is_ok() {
            Box::new(can)
        } else {
            let mut sim = VirtualCanInterface::new();
            let _ = sim.open().await;
            Box::new(sim)
        }
    }
}

pub fn load_mod_input(input: &str) -> Result<SterngateMod> {
    let content = if input == "-" {
        use std::io::Read;
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else if std::path::Path::new(input).is_file() {
        std::fs::read_to_string(input)?
    } else {
        input.to_string()
    };

    if content.contains("BEGIN STERNGATE COMMUNITY MOD") {
        decode_from_armor(&content).map_err(|e| anyhow::anyhow!("ASCII armor decode failed: {}", e))
    } else {
        SterngateMod::from_json(&content).map_err(|e| anyhow::anyhow!("JSON decode failed: {}", e))
    }
}

pub fn parse_hex_bytes(s: &str) -> Result<Vec<u8>> {
    let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if !clean.len().is_multiple_of(2) {
        anyhow::bail!("Hex string must have an even number of characters: '{}'", s);
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|e| anyhow::anyhow!("Invalid hex byte at index {}: {}", i, e))
        })
        .collect()
}
