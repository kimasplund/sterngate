use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FlashState {
    Idle,
    Staging,
    Verifying,
    Locked,
    SessionExtended,
    SecurityUnlocked,
    BusSilenced,
    SessionProgramming,
    Erasing,
    Transferring,
    TransferExited,
    VerifyingChecksum,
    ResettingEcu,
    Completed,
    Failed,
    Aborted,
}

impl FlashState {
    pub fn is_locked(&self) -> bool {
        !matches!(
            self,
            FlashState::Idle | FlashState::Completed | FlashState::Failed | FlashState::Aborted
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlashProgress {
    pub state: FlashState,
    pub percentage: u8,
    pub current_block: usize,
    pub total_blocks: usize,
    pub bytes_written: usize,
    pub total_bytes: usize,
    pub log: String,
    pub error_message: Option<String>,
}

impl Default for FlashProgress {
    fn default() -> Self {
        Self {
            state: FlashState::Idle,
            percentage: 0,
            current_block: 0,
            total_blocks: 0,
            bytes_written: 0,
            total_bytes: 0,
            log: "Idle - No flash in progress".to_string(),
            error_message: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlashPackageManifest {
    pub target_module: String,
    pub expected_hw_id: String,
    pub expected_sw_id: String,
    pub sha256_checksum: String,
    pub crc32_checksum: u32,
    pub flash_start_address: u32,
    pub flash_length: u32,
    pub block_size: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreFlightReport {
    pub passed: bool,
    pub battery_voltage: f64,
    pub min_voltage_required: f64,
    pub hw_id_match: bool,
    pub checksum_match: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareSignatures {
    pub bosch_hw_id: Option<String>,
    pub bosch_sw_id: Option<String>,
    pub oem_part_number: Option<String>,
    pub project_name: Option<String>,
    pub file_size_bytes: usize,
    pub sha256_checksum: String,
    pub crc32_checksum: u32,
}

impl FirmwareSignatures {
    pub fn extract(data: &[u8]) -> Self {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(data);
        let sha256_checksum = format!("{:x}", hasher.finalize());
        let crc32_checksum = crc32fast::hash(data);

        let mut bosch_hw_id = None;
        let mut bosch_sw_id = None;
        let mut oem_part_number = None;
        let mut project_name = None;

        let len = data.len();
        let scan_limit = len.min(4 * 1024 * 1024);

        // 1. Scan for Bosch HW (10 digits starting with 0281 or 0261)
        // 2. Scan for Bosch SW (10 digits starting with 1037 or 1039)
        let mut i = 0;
        while i + 10 <= scan_limit {
            let slice = &data[i..i + 10];
            if bosch_hw_id.is_none()
                && (slice.starts_with(b"0281") || slice.starts_with(b"0261"))
                && slice.iter().all(|b| b.is_ascii_digit())
            {
                if let Ok(s) = std::str::from_utf8(slice) {
                    bosch_hw_id = Some(s.to_string());
                }
            } else if bosch_sw_id.is_none()
                && (slice.starts_with(b"1037") || slice.starts_with(b"1039"))
                && slice.iter().all(|b| b.is_ascii_digit())
            {
                if let Ok(s) = std::str::from_utf8(slice) {
                    bosch_sw_id = Some(s.to_string());
                }
            }
            i += 1;
        }

        // 3. Scan for Mercedes OEM part number (e.g. "A 646 150 08 79" (15 bytes) or "A6461500879" (11 bytes))
        i = 0;
        while i + 15 <= scan_limit {
            if data[i] == b'A' && data[i + 1] == b' ' {
                let s = &data[i..i + 15];
                if oem_part_number.is_none()
                    && s[2].is_ascii_digit()
                    && s[3].is_ascii_digit()
                    && s[4].is_ascii_digit()
                    && s[5] == b' '
                    && s[6].is_ascii_digit()
                    && s[7].is_ascii_digit()
                    && s[8].is_ascii_digit()
                    && s[9] == b' '
                    && s[10].is_ascii_digit()
                    && s[11].is_ascii_digit()
                    && s[12] == b' '
                    && s[13].is_ascii_digit()
                    && s[14].is_ascii_digit()
                {
                    if let Ok(num) = std::str::from_utf8(s) {
                        oem_part_number = Some(num.trim().to_string());
                    }
                }
            }
            i += 1;
        }

        if oem_part_number.is_none() {
            i = 0;
            while i + 11 <= scan_limit {
                if data[i] == b'A' && data[i + 1..i + 11].iter().all(|b| b.is_ascii_digit()) {
                    if let Ok(num) = std::str::from_utf8(&data[i..i + 11]) {
                        oem_part_number = Some(num.to_string());
                        break;
                    }
                }
                i += 1;
            }
        }

        // 4. Scan for project name (e.g. "CR4-", "CR3-", "EDC16", "EDC17", "ME9.")
        let prefixes: &[&[u8]] = &[
            b"CR4-", b"CR3-", b"CR5-", b"CR6-", b"EDC16", b"EDC17", b"ME9.", b"CRD2",
        ];
        for &prefix in prefixes {
            if let Some(pos) = data[..scan_limit]
                .windows(prefix.len())
                .position(|w| w == prefix)
            {
                let mut end = pos;
                while end < len
                    && end < pos + 40
                    && data[end] >= 0x20
                    && data[end] <= 0x7E
                    && data[end] != b';'
                    && data[end] != b'\0'
                {
                    end += 1;
                }
                if end > pos + prefix.len() {
                    if let Ok(p) = std::str::from_utf8(&data[pos..end]) {
                        project_name = Some(p.trim().to_string());
                        break;
                    }
                }
            }
        }

        Self {
            bosch_hw_id,
            bosch_sw_id,
            oem_part_number,
            project_name,
            file_size_bytes: len,
            sha256_checksum,
            crc32_checksum,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RomCompatibilityVerdict {
    Match,
    CalibrationUpdate,
    HardwareMismatch,
    EngineMismatch,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RomInspectionReport {
    pub signatures: FirmwareSignatures,
    pub ecu_hw_id: Option<String>,
    pub ecu_sw_id: Option<String>,
    pub ecu_oem_num: Option<String>,
    pub verdict: RomCompatibilityVerdict,
    pub can_flash: bool,
    pub risk_explanation: String,
}

/// Entry representing a discovered local firmware binary or container
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareVaultEntry {
    pub file_path: String,
    pub filename: String,
    pub file_size_bytes: usize,
    pub format: String,
    pub signatures: FirmwareSignatures,
}

/// Recommendation generated when comparing connected vehicle ECU against local vault
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareUpgradeRecommendation {
    pub target_module: String,
    pub live_hw_id: String,
    pub live_sw_id: String,
    pub recommended_file: FirmwareVaultEntry,
    pub reason: String,
    pub can_stage: bool,
}

/// Local Firmware Vault scanner and manager
pub struct FirmwareVault;

impl FirmwareVault {
    /// Scan a directory recursively for firmware binaries (.bin, .rom, .cff, .smr-f, .fls)
    /// Default firmware vault root (`firmware_vault`), overridable by the
    /// `STERNGATE_VAULT_ROOT` environment variable, mirroring how the ECU
    /// catalog, DTC database and vehicle garage resolve their locations.
    pub fn default_root() -> std::path::PathBuf {
        if let Ok(env_path) = std::env::var("STERNGATE_VAULT_ROOT") {
            return std::path::PathBuf::from(env_path);
        }
        std::path::PathBuf::from("firmware_vault")
    }

    pub fn scan_directory(dir: impl AsRef<std::path::Path>) -> Vec<FirmwareVaultEntry> {
        let mut results = Vec::new();
        Self::scan_recursive(dir.as_ref(), &mut results);
        results
    }

    fn scan_recursive(dir: &std::path::Path, results: &mut Vec<FirmwareVaultEntry>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    Self::scan_recursive(&p, results);
                } else if p.is_file() {
                    let ext = p
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if matches!(ext.as_str(), "bin" | "rom" | "cff" | "smr-f" | "fls") {
                        if let Ok(data) = std::fs::read(&p) {
                            let sigs = FirmwareSignatures::extract(&data);
                            let filename = p
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let format = match ext.as_str() {
                                "bin" | "rom" => "Raw Flash Binary (.bin)",
                                "cff" => "Caesar Flash Container (.cff)",
                                "smr-f" => "Modular Flash Container (.smr-f)",
                                _ => "Binary Calibration (.fls)",
                            }
                            .to_string();

                            results.push(FirmwareVaultEntry {
                                file_path: p.to_string_lossy().to_string(),
                                filename,
                                file_size_bytes: data.len(),
                                format,
                                signatures: sigs,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Check if any file in the vault matches the connected ECU hardware, and whether it's an upgrade
    pub fn find_upgrade_recommendation(
        entries: &[FirmwareVaultEntry],
        live_hw_id: &str,
        live_sw_id: &str,
    ) -> Option<FirmwareUpgradeRecommendation> {
        for entry in entries {
            if let Some(entry_hw) = &entry.signatures.bosch_hw_id {
                let check_len = entry_hw.len().min(live_hw_id.len());
                if check_len >= 8
                    && entry_hw[..check_len].eq_ignore_ascii_case(&live_hw_id[..check_len])
                {
                    if let Some(entry_sw) = &entry.signatures.bosch_sw_id {
                        if !entry_sw.eq_ignore_ascii_case(live_sw_id) {
                            return Some(FirmwareUpgradeRecommendation {
                                target_module: "EDC16".into(),
                                live_hw_id: live_hw_id.to_string(),
                                live_sw_id: live_sw_id.to_string(),
                                recommended_file: entry.clone(),
                                reason: format!(
                                    "Local calibration {} supersedes connected calibration {}",
                                    entry_sw, live_sw_id
                                ),
                                can_stage: true,
                            });
                        }
                    }
                }
            }
        }
        None
    }
}
