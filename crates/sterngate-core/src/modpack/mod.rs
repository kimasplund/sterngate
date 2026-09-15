pub mod armor;
pub mod fec;

use crate::calibrator::map::MapProvenance;
use crate::error::{Result, SterngateError};
use fec::{FecStatus, ReedSolomonCodec, DEFAULT_DATA_BLOCK_LEN, DEFAULT_PARITY_LEN};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Minimum measured battery voltage for any package that writes flash memory
/// (`PatchFlashMap`, `DtcMask`). Mirrors the flashing worker's erase interlock.
pub const FLASH_WRITE_MIN_VOLTAGE: f64 = 12.5;

/// Category classification for community mods
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModCategory {
    Performance,
    Transmission,
    Comfort,
    Lighting,
    Brakes,
    Emissions,
    Retrofit,
    Diagnostics,
}

impl ModCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Performance => "Performance Tuning",
            Self::Transmission => "Transmission Calibration",
            Self::Comfort => "Vehicle Comfort",
            Self::Lighting => "Lighting & Visibility",
            Self::Brakes => "Brake Systems",
            Self::Emissions => "Emissions Optimization",
            Self::Retrofit => "Equipment Retrofit",
            Self::Diagnostics => "Diagnostics & Fault Management",
        }
    }
}

/// Risk level rating
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModRiskLevel {
    Low,
    Moderate,
    High,
}

impl ModRiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "Low (Reversible UI/Comfort toggle)",
            Self::Moderate => "Moderate (Powertrain adaptation / Variant coding)",
            Self::High => "High (Safety critical / High voltage or braking)",
        }
    }
}

/// Metadata describing author, purpose, and versioning of the mod
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModMetadata {
    pub mod_id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub category: ModCategory,
    pub risk_level: ModRiskLevel,
    #[serde(default)]
    pub instructions: Option<String>,
    pub created_at: String,
}

/// Strict vehicle and ECU fingerprint filter
/// Ensures the mod is only applied to the exact compatible vehicle configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModTargetFilter {
    /// Supported chassis models (e.g. ["W211", "S211", "C219"]). Matches VIN prefix.
    pub chassis: Vec<String>,
    /// Target ECU module identifier (e.g. "EDC16", "EGS52", "SAM-F", "KI")
    pub ecu_name: String,
    /// Diagnostic CAN request ID (e.g. 0x7E0)
    pub tx_id: u32,
    /// Diagnostic CAN response ID (e.g. 0x7E8)
    pub rx_id: u32,
    /// Optional whitelist of Bosch/Continental hardware part numbers (e.g. ["0281013854"])
    #[serde(default)]
    pub compatible_hw_ids: Vec<String>,
    /// Optional whitelist of software calibration versions
    #[serde(default)]
    pub compatible_sw_ids: Vec<String>,
    /// Minimum required battery voltage before executing (defaults to 12.0V)
    #[serde(default = "default_min_voltage")]
    pub min_battery_voltage: f64,
    /// Requires ignition ON and engine OFF
    #[serde(default = "default_engine_off")]
    pub requires_engine_off: bool,
}

fn default_min_voltage() -> f64 {
    12.0
}

fn default_engine_off() -> bool {
    true
}

impl ModTargetFilter {
    /// Check if target chassis matches vehicle VIN (e.g. "WDB2112061A..." matches "W211" or "211")
    pub fn matches_chassis(&self, vin: &str) -> bool {
        if self.chassis.is_empty() {
            return true;
        }
        let clean_vin = vin.to_uppercase();
        self.chassis.iter().any(|c| {
            let clean_c = c.to_uppercase();
            if clean_vin.contains(&clean_c) {
                return true;
            }
            let digits: String = clean_c.chars().filter(|ch| ch.is_ascii_digit()).collect();
            if digits.len() == 3 && clean_vin.contains(&digits) {
                return true;
            }
            false
        })
    }

    /// Check if target hardware ID is compatible
    pub fn matches_hardware(&self, live_hw_id: &str) -> bool {
        if self.compatible_hw_ids.is_empty() {
            return true;
        }
        self.compatible_hw_ids
            .iter()
            .any(|hw| live_hw_id.contains(hw))
    }
}

/// Action steps executed by the mod
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModAction {
    /// Write Data Identifier (DID) with optional bitmask preservation and precondition check
    WriteDid {
        did: u16,
        data: Vec<u8>,
        /// Optional bitmask (1 = replace bit with mod data, 0 = keep live vehicle bit)
        bitmask: Option<Vec<u8>>,
        /// Optional expected original bytes (aborts if live vehicle does not match)
        expected_original_data: Option<Vec<u8>>,
        description: String,
    },
    /// Execute a UDS / KWP diagnostic routine (Service 0x31)
    Routine {
        routine_id: u16,
        subfunction: u8,
        data: Vec<u8>,
        description: String,
    },
    /// Patch an ECU flash calibration map (e.g. Torque Limiter, Boost Target, SVBL)
    PatchFlashMap {
        map_name: String,
        address_offset: u32,
        data: Vec<u8>,
        /// Optional expected original bytes at offset (precondition check)
        expected_original_data: Option<Vec<u8>>,
        description: String,
        /// Origin of `address_offset`/`data`. Absent in legacy packages, which
        /// deserialize to `Unverified` and are refused by the runner.
        #[serde(default, skip_serializing_if = "MapProvenance::is_unverified")]
        provenance: MapProvenance,
    },
    /// Suppress or disable a specific Diagnostic Trouble Code in ECU flash memory
    DtcMask {
        p_code: String,
        address_offset: u32,
        original_mask: u8,
        disable_mask: u8,
        description: String,
        /// Origin of `address_offset`. Same rules as `PatchFlashMap`.
        #[serde(default, skip_serializing_if = "MapProvenance::is_unverified")]
        provenance: MapProvenance,
    },
}

/// Cryptographic and forward error correction integrity block
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModIntegrity {
    pub payload_crc32: u32,
    pub payload_sha256: String,
    pub fec_scheme: String,
    pub fec_parity_bytes: Vec<u8>,
    pub block_size: usize,
    pub parity_size: usize,
}

/// The complete shareable Sterngate Community Mod package
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SterngateMod {
    pub metadata: ModMetadata,
    pub target: ModTargetFilter,
    pub actions: Vec<ModAction>,
    #[serde(default)]
    pub rollback_actions: Vec<ModAction>,
    pub integrity: ModIntegrity,
}

/// Detailed result of mod inspection and validation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModValidationReport {
    pub is_valid: bool,
    pub fec_status: FecStatus,
    pub crc32_verified: bool,
    pub sha256_verified: bool,
    pub matched_vehicle: bool,
    pub compatibility_notes: Vec<String>,
    pub warning_messages: Vec<String>,
}

impl SterngateMod {
    /// Serialize mod package to formatted JSON string
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            SterngateError::ProfileError(format!("Failed serializing mod to JSON: {}", e))
        })
    }

    /// Parse mod package from JSON string
    pub fn from_json(json_str: &str) -> Result<Self> {
        serde_json::from_str(json_str)
            .map_err(|e| SterngateError::ProfileError(format!("Failed parsing mod JSON: {}", e)))
    }

    /// Create a new community mod and automatically calculate Reed-Solomon FEC parity and checksums
    pub fn create(
        metadata: ModMetadata,
        target: ModTargetFilter,
        actions: Vec<ModAction>,
        rollback_actions: Vec<ModAction>,
    ) -> Result<Self> {
        let canonical_payload = Self::canonical_payload_bytes(&actions, &rollback_actions)?;

        let payload_crc32 = crc32fast::hash(&canonical_payload);

        let mut hasher = Sha256::new();
        hasher.update(&canonical_payload);
        let payload_sha256 = format!("{:x}", hasher.finalize());

        let codec = ReedSolomonCodec::default_codec();
        let fec_parity_bytes = codec.encode(&canonical_payload);

        let integrity = ModIntegrity {
            payload_crc32,
            payload_sha256,
            fec_scheme: "ReedSolomon_GF256".into(),
            fec_parity_bytes,
            block_size: DEFAULT_DATA_BLOCK_LEN,
            parity_size: DEFAULT_PARITY_LEN,
        };

        Ok(Self {
            metadata,
            target,
            actions,
            rollback_actions,
            integrity,
        })
    }

    /// Canonical serialization of actions and rollback steps for reproducible checksumming
    pub fn canonical_payload_bytes(
        actions: &[ModAction],
        rollback_actions: &[ModAction],
    ) -> Result<Vec<u8>> {
        let pair = (actions, rollback_actions);
        serde_json::to_vec(&pair).map_err(|e| {
            SterngateError::ProfileError(format!("Failed serializing mod payload: {}", e))
        })
    }

    /// Verify package integrity and repair any corrupted bytes using Reed-Solomon FEC.
    /// If corrupted bytes are repaired, `actions` and `rollback_actions` are updated in-place.
    pub fn verify_and_repair(&mut self) -> Result<ModValidationReport> {
        let mut payload_bytes =
            Self::canonical_payload_bytes(&self.actions, &self.rollback_actions)?;

        let codec = ReedSolomonCodec::new(self.integrity.block_size, self.integrity.parity_size);
        let fec_status =
            codec.decode_and_repair(&mut payload_bytes, &self.integrity.fec_parity_bytes);

        match &fec_status {
            FecStatus::Unrecoverable { reason } => {
                return Ok(ModValidationReport {
                    is_valid: false,
                    fec_status: fec_status.clone(),
                    crc32_verified: false,
                    sha256_verified: false,
                    matched_vehicle: false,
                    compatibility_notes: vec![],
                    warning_messages: vec![format!("Mod corruption unrecoverable: {}", reason)],
                });
            }
            FecStatus::Repaired { .. } => {
                // Deserialize repaired actions back into struct
                let (repaired_actions, repaired_rollback): (Vec<ModAction>, Vec<ModAction>) =
                    serde_json::from_slice(&payload_bytes).map_err(|e| {
                        SterngateError::ProfileError(format!(
                            "Failed parsing repaired mod payload: {}",
                            e
                        ))
                    })?;
                self.actions = repaired_actions;
                self.rollback_actions = repaired_rollback;
            }
            FecStatus::Intact => {}
        }

        // Check CRC32
        let calculated_crc = crc32fast::hash(&payload_bytes);
        let crc32_verified = calculated_crc == self.integrity.payload_crc32;

        // Check SHA256
        let mut hasher = Sha256::new();
        hasher.update(&payload_bytes);
        let calculated_sha = format!("{:x}", hasher.finalize());
        let sha256_verified = calculated_sha == self.integrity.payload_sha256;

        let is_valid = crc32_verified && sha256_verified;

        let mut warning_messages = Vec::new();
        if !crc32_verified {
            warning_messages.push(format!(
                "CRC32 mismatch: expected 0x{:08X}, calculated 0x{:08X}",
                self.integrity.payload_crc32, calculated_crc
            ));
        }
        if !sha256_verified {
            warning_messages.push(format!(
                "SHA256 mismatch: expected {}, calculated {}",
                self.integrity.payload_sha256, calculated_sha
            ));
        }

        Ok(ModValidationReport {
            is_valid,
            fec_status,
            crc32_verified,
            sha256_verified,
            matched_vehicle: false,
            compatibility_notes: Vec::new(),
            warning_messages,
        })
    }

    /// Check compatibility against a live vehicle profile / VIN and ECU Hardware ID
    pub fn check_compatibility(
        &self,
        vin: Option<&str>,
        live_hw_id: Option<&str>,
        battery_voltage: Option<f64>,
    ) -> ModValidationReport {
        let mut report = self
            .clone()
            .verify_and_repair()
            .unwrap_or_else(|e| ModValidationReport {
                is_valid: false,
                fec_status: FecStatus::Unrecoverable {
                    reason: e.to_string(),
                },
                crc32_verified: false,
                sha256_verified: false,
                matched_vehicle: false,
                compatibility_notes: Vec::new(),
                warning_messages: vec![format!("Verification error: {}", e)],
            });

        if !report.is_valid {
            return report;
        }

        let mut matched = true;

        if let Some(v) = vin {
            if self.target.matches_chassis(v) {
                report
                    .compatibility_notes
                    .push(format!("✓ Chassis matches VIN ({})", v));
            } else {
                matched = false;
                report.warning_messages.push(format!(
                    "Chassis mismatch: Mod targets {:?}, but connected vehicle is VIN {}",
                    self.target.chassis, v
                ));
            }
        }

        if let Some(hw) = live_hw_id {
            if self.target.matches_hardware(hw) {
                report
                    .compatibility_notes
                    .push(format!("✓ ECU Hardware ID compatible ({})", hw));
            } else {
                matched = false;
                report.warning_messages.push(format!(
                    "ECU Hardware mismatch: Mod targets {:?}, but connected ECU hardware is {}",
                    self.target.compatible_hw_ids, hw
                ));
            }
        }

        if let Some(volts) = battery_voltage {
            if volts >= self.target.min_battery_voltage {
                report.compatibility_notes.push(format!(
                    "✓ Battery voltage sufficient ({:.1}V >= {:.1}V)",
                    volts, self.target.min_battery_voltage
                ));
            } else {
                matched = false;
                report.warning_messages.push(format!(
                    "Battery voltage too low: measured {:.1}V, required >= {:.1}V",
                    volts, self.target.min_battery_voltage
                ));
            }
        }

        report.matched_vehicle = matched;
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibrator::map::MapProvenance;

    pub(crate) fn sample_metadata() -> ModMetadata {
        ModMetadata {
            mod_id: "w211-test".into(),
            name: "W211 Test".into(),
            version: "1.0.0".into(),
            author: "TunerKim".into(),
            description: "test package".into(),
            category: ModCategory::Performance,
            risk_level: ModRiskLevel::Moderate,
            instructions: None,
            created_at: "2026-09-15T12:00:00Z".into(),
        }
    }

    pub(crate) fn sample_target(min_battery_voltage: f64) -> ModTargetFilter {
        ModTargetFilter {
            chassis: vec!["W211".into()],
            ecu_name: "EDC16".into(),
            tx_id: 0x7E0,
            rx_id: 0x7E8,
            compatible_hw_ids: vec![],
            compatible_sw_ids: vec![],
            min_battery_voltage,
            requires_engine_off: true,
        }
    }

    pub(crate) fn flash_patch(provenance: MapProvenance) -> ModAction {
        ModAction::PatchFlashMap {
            map_name: "Torque Limiter".into(),
            address_offset: 0x1C1000,
            data: vec![0x0B, 0xB8, 0x10],
            expected_original_data: Some(vec![0x00, 0x00, 0x00]),
            description: "test patch".into(),
            provenance,
        }
    }

    #[test]
    fn provenance_defaults_to_unverified_and_keeps_legacy_integrity() {
        // A package whose actions carry the default provenance serializes
        // exactly like a package written before the field existed.
        let m = SterngateMod::create(
            sample_metadata(),
            sample_target(12.5),
            vec![flash_patch(MapProvenance::Unverified)],
            vec![],
        )
        .unwrap();
        let json = m.to_json().unwrap();
        assert!(!json.contains("provenance"));

        let mut parsed = SterngateMod::from_json(&json).unwrap();
        match &parsed.actions[0] {
            ModAction::PatchFlashMap { provenance, .. } => {
                assert_eq!(*provenance, MapProvenance::Unverified);
            }
            other => panic!("unexpected action {other:?}"),
        }
        assert!(parsed.verify_and_repair().unwrap().is_valid);

        // A labelled package round-trips its label.
        let m2 = SterngateMod::create(
            sample_metadata(),
            sample_target(12.5),
            vec![flash_patch(MapProvenance::Synthetic)],
            vec![],
        )
        .unwrap();
        let json2 = m2.to_json().unwrap();
        assert!(json2.contains("\"provenance\": \"synthetic\""));
        assert!(
            SterngateMod::from_json(&json2)
                .unwrap()
                .verify_and_repair()
                .unwrap()
                .is_valid
        );
    }
}
