pub fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if !clean.len().is_multiple_of(2) {
        return Err("Hex string must have an even length".into());
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|e| format!("Invalid hex byte at position {}: {}", i, e))
        })
        .collect()
}
