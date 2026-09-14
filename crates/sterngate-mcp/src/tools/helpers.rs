use serde_json::Value;
use sterngate_core::{decode_from_armor, SterngateMod};

pub fn parse_mod_content(input: &str) -> Result<SterngateMod, String> {
    let content = if std::path::Path::new(input).is_file() {
        std::fs::read_to_string(input)
            .map_err(|e| format!("Failed reading mod file '{}': {}", input, e))?
    } else {
        input.to_string()
    };

    if content.contains("BEGIN STERNGATE COMMUNITY MOD") {
        decode_from_armor(&content).map_err(|e| format!("Armor decode failed: {}", e))
    } else {
        SterngateMod::from_json(&content).map_err(|e| format!("JSON parse failed: {}", e))
    }
}

pub fn parse_hex_slice(s: &str) -> Result<Vec<u8>, String> {
    let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if !clean.len().is_multiple_of(2) {
        return Err(format!("Hex string must have an even length: '{}'", s));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&clean[i..i + 2], 16).map_err(|e| format!("Invalid hex byte: {}", e))
        })
        .collect()
}

pub fn load_rom_bytes_mcp(arguments: &Value) -> Result<Vec<u8>, String> {
    if let Some(path_str) = arguments.get("rom_path").and_then(|v| v.as_str()) {
        std::fs::read(path_str)
            .map_err(|e| format!("Failed reading ROM file '{}': {}", path_str, e))
    } else if let Some(b64_str) = arguments.get("rom_base64").and_then(|v| v.as_str()) {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(b64_str)
            .map_err(|e| format!("Invalid base64 ROM payload: {}", e))
    } else {
        Err("Either 'rom_path' or 'rom_base64' must be provided in arguments".to_string())
    }
}
