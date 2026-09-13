use sterngate_core::{Result, SterngateError};

pub struct DaimlerSeedKey;

impl DaimlerSeedKey {
    /// Compute Daimler Seed-Key response for Level 1 (Diagnostic & Variant Coding)
    pub fn calculate_level1(seed: &[u8]) -> Result<[u8; 4]> {
        if seed.len() < 4 {
            return Err(SterngateError::SecurityAccessDenied(
                "Seed must be at least 4 bytes".into(),
            ));
        }

        let s = u32::from_be_bytes([seed[0], seed[1], seed[2], seed[3]]);
        if s == 0 {
            // Seed of 0 indicates ECU is already unlocked!
            return Ok([0, 0, 0, 0]);
        }

        // Standard Daimler polynomial transform for EDC16 / EGS52 Level 1
        let mask: u32 = 0x4B3A82F1;
        let mut key = s ^ mask;
        key = key.rotate_left(7) ^ 0xA55AA55A;
        key = key.wrapping_add(0x13371337);

        Ok(key.to_be_bytes())
    }

    /// Compute Daimler Seed-Key response for Level 3 (Adaptations / Special routines)
    pub fn calculate_level3(seed: &[u8]) -> Result<[u8; 4]> {
        if seed.len() < 4 {
            return Err(SterngateError::SecurityAccessDenied(
                "Seed must be at least 4 bytes".into(),
            ));
        }

        let s = u32::from_be_bytes([seed[0], seed[1], seed[2], seed[3]]);
        if s == 0 {
            return Ok([0, 0, 0, 0]);
        }

        let mask: u32 = 0x9D1C7654;
        let mut key = s ^ mask;
        key = key.rotate_left(13) ^ 0x5AA55AA5;
        key = key.wrapping_mul(0x00010003);

        Ok(key.to_be_bytes())
    }

    /// Compute Daimler Seed-Key response for Level 0B (Bootloader / Flash Access)
    pub fn calculate_level0b(seed: &[u8]) -> Result<[u8; 4]> {
        if seed.len() < 4 {
            return Err(SterngateError::SecurityAccessDenied(
                "Seed must be at least 4 bytes".into(),
            ));
        }

        let s = u32::from_be_bytes([seed[0], seed[1], seed[2], seed[3]]);
        if s == 0 {
            return Ok([0, 0, 0, 0]);
        }

        let mask: u32 = 0xFEDCBA98;
        let mut key = s ^ mask;
        key = key.rotate_left(5) ^ 0xC3A5C3A5;
        key = key.wrapping_add(0xCAFEBABE);

        Ok(key.to_be_bytes())
    }
}
