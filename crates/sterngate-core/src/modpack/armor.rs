//! ASCII Armored serialization and fault-tolerant parser for community mods.
//!
//! Enables sharing one-click tuning mods across web forums, Discord, Telegram,
//! and pastebins without corruption from markdown formatting or line-ending glitches.

use super::SterngateMod;
use crate::error::{Result, SterngateError};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

pub const ARMOR_HEADER: &str = "-----BEGIN STERNGATE COMMUNITY MOD-----";
pub const ARMOR_FOOTER: &str = "-----END STERNGATE COMMUNITY MOD-----";

/// Encode a SterngateMod into a copy-paste friendly ASCII Armored text block
pub fn encode_to_armor(modpack: &SterngateMod) -> Result<String> {
    let json_bytes = serde_json::to_vec(modpack).map_err(|e| {
        SterngateError::ProfileError(format!("Failed serializing mod to JSON: {}", e))
    })?;

    let b64 = BASE64.encode(&json_bytes);

    let mut out = String::new();
    out.push_str(ARMOR_HEADER);
    out.push('\n');
    out.push_str(&format!("Mod-ID: {}\n", modpack.metadata.mod_id));
    out.push_str(&format!("Name: {}\n", modpack.metadata.name));
    out.push_str(&format!("Author: {}\n", modpack.metadata.author));
    out.push_str(&format!(
        "Target-Chassis: {}\n",
        modpack.target.chassis.join(", ")
    ));
    out.push_str(&format!(
        "Target-ECU: {} (0x{:03X})\n",
        modpack.target.ecu_name, modpack.target.tx_id
    ));
    out.push_str(&format!("CRC32: {:08X}\n", modpack.integrity.payload_crc32));
    out.push_str(&format!("SHA256: {}\n", modpack.integrity.payload_sha256));
    out.push_str(&format!("FEC: {}\n\n", modpack.integrity.fec_scheme));

    // Chunk base64 into 64-character lines
    for chunk in b64.as_bytes().chunks(64) {
        if let Ok(line) = std::str::from_utf8(chunk) {
            out.push_str(line);
            out.push('\n');
        }
    }

    out.push_str(ARMOR_FOOTER);
    out.push('\n');

    Ok(out)
}

/// Parse and decode an ASCII Armored block or plain JSON text
pub fn decode_from_armor(input: &str) -> Result<SterngateMod> {
    let clean_input = input.trim();

    // 1. Direct JSON check
    if clean_input.starts_with('{') && clean_input.ends_with('}') {
        let mut modpack: SterngateMod = serde_json::from_str(clean_input).map_err(|e| {
            SterngateError::ProfileError(format!("Invalid JSON mod package: {}", e))
        })?;
        let report = modpack.verify_and_repair()?;
        if !report.is_valid {
            return Err(SterngateError::ProfileError(format!(
                "Mod package integrity check failed: {:?}",
                report.warning_messages
            )));
        }
        return Ok(modpack);
    }

    // 2. Strip markdown backticks if copied inside code fences
    let without_fences = clean_input
        .trim_start_matches("```text")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // 3. Find armor delimiters
    let start_idx = without_fences.find(ARMOR_HEADER).ok_or_else(|| {
        SterngateError::ProfileError(
            "Missing '-----BEGIN STERNGATE COMMUNITY MOD-----' header".into(),
        )
    })?;

    let end_idx = without_fences.find(ARMOR_FOOTER).ok_or_else(|| {
        SterngateError::ProfileError(
            "Missing '-----END STERNGATE COMMUNITY MOD-----' footer".into(),
        )
    })?;

    if end_idx <= start_idx {
        return Err(SterngateError::ProfileError(
            "Corrupted armor delimiters: footer precedes header".into(),
        ));
    }

    let body = &without_fences[start_idx + ARMOR_HEADER.len()..end_idx];

    let mut b64_acc = String::new();
    let mut past_headers = false;
    let mut seen_header = false;

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if seen_header {
                past_headers = true;
            }
            continue;
        }

        if !past_headers {
            if trimmed.contains(": ") {
                seen_header = true;
                continue;
            }
            past_headers = true;
        }

        b64_acc.push_str(trimmed);
    }

    if b64_acc.is_empty() {
        return Err(SterngateError::ProfileError(
            "Empty mod payload inside armor block".into(),
        ));
    }

    let json_bytes = BASE64.decode(b64_acc.as_bytes()).map_err(|e| {
        SterngateError::ProfileError(format!("Base64 decoding failed for mod payload: {}", e))
    })?;

    let mut modpack: SterngateMod = serde_json::from_slice(&json_bytes)
        .map_err(|e| SterngateError::ProfileError(format!("Invalid mod package JSON: {}", e)))?;

    // Automatically verify integrity and apply Reed-Solomon repair if needed
    let report = modpack.verify_and_repair()?;
    if !report.is_valid {
        return Err(SterngateError::ProfileError(format!(
            "Mod package integrity check failed: {:?}",
            report.warning_messages
        )));
    }

    Ok(modpack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modpack::{ModAction, ModCategory, ModMetadata, ModRiskLevel, ModTargetFilter};

    fn sample_mod() -> SterngateMod {
        let metadata = ModMetadata {
            mod_id: "w211-om642-throttle-agility".into(),
            name: "W211 OM642 Agility Throttle Response".into(),
            version: "1.0.0".into(),
            author: "TunerKev".into(),
            description: "Sharpens low-RPM accelerator pedal mapping and removes lag".into(),
            category: ModCategory::Performance,
            risk_level: ModRiskLevel::Moderate,
            instructions: Some("Ignition ON, Engine OFF".into()),
            created_at: "2026-09-14T12:00:00Z".into(),
        };

        let target = ModTargetFilter {
            chassis: vec!["W211".into(), "S211".into()],
            ecu_name: "EDC16".into(),
            tx_id: 0x7E0,
            rx_id: 0x7E8,
            compatible_hw_ids: vec!["0281013854".into()],
            compatible_sw_ids: vec![],
            min_battery_voltage: 12.0,
            requires_engine_off: true,
        };

        let actions = vec![ModAction::WriteDid {
            did: 0x0110,
            data: vec![0x01, 0x2C], // 300 km/h
            bitmask: None,
            expected_original_data: None,
            description: "Speed limiter threshold adjustment".into(),
        }];

        SterngateMod::create(metadata, target, actions, vec![]).unwrap()
    }

    #[test]
    fn test_armor_encode_and_decode_roundtrip() {
        let original = sample_mod();
        let armored = encode_to_armor(&original).unwrap();
        assert!(armored.contains(ARMOR_HEADER));
        assert!(armored.contains(ARMOR_FOOTER));
        assert!(armored.contains("TunerKev"));

        let decoded = decode_from_armor(&armored).unwrap();
        assert_eq!(decoded.metadata.mod_id, original.metadata.mod_id);
        assert_eq!(decoded.target.tx_id, original.target.tx_id);
        assert_eq!(
            decoded.integrity.payload_crc32,
            original.integrity.payload_crc32
        );
    }

    #[test]
    fn test_armor_with_markdown_fences_and_whitespace() {
        let original = sample_mod();
        let armored = encode_to_armor(&original).unwrap();
        let wrapped_in_markdown = format!("```text\r\n   {}\r\n   \r\n```", armored);

        let decoded = decode_from_armor(&wrapped_in_markdown).unwrap();
        assert_eq!(decoded.metadata.mod_id, original.metadata.mod_id);
    }

    #[test]
    fn decode_from_armor_rejects_corrupt_plain_json() {
        let mut m = sample_mod();
        m.integrity.payload_crc32 ^= 1;
        let json = m.to_json().unwrap();
        assert!(decode_from_armor(&json).is_err());
    }
}
