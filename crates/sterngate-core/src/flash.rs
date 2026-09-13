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
