use crate::error::{Result, SterngateError};
use crate::profile::VehicleProfile;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub command_id: String,
    pub timestamp_ms: u64,
    pub ttl_ms: u64,
    pub idempotency_key: String,
    pub target_module: String,
    pub service: u8,
    pub did: Option<u16>,
    pub payload_len: usize,
    pub payload_crc32: u32,
    pub payload: Vec<u8>,
}

impl CommandEnvelope {
    pub fn new(target_module: &str, service: u8, did: Option<u16>, payload: Vec<u8>) -> Self {
        let payload_len = payload.len();
        let payload_crc32 = crc32fast::hash(&payload);
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;

        Self {
            command_id: Uuid::new_v4().to_string(),
            timestamp_ms: now_ms,
            ttl_ms: 10_000, // 10 seconds default TTL for web commands
            idempotency_key: Uuid::new_v4().to_string(),
            target_module: target_module.to_string(),
            service,
            did,
            payload_len,
            payload_crc32,
            payload,
        }
    }

    pub fn with_ttl(mut self, ttl_ms: u64) -> Self {
        self.ttl_ms = ttl_ms;
        self
    }

    pub fn with_idempotency_key(mut self, key: &str) -> Self {
        self.idempotency_key = key.to_string();
        self
    }

    /// Verifies that the payload arrived completely and uncorrupted
    pub fn validate_integrity(&self) -> Result<()> {
        // 1. Length check (detects truncated arrivals over dropped sockets)
        if self.payload.len() != self.payload_len {
            return Err(SterngateError::ParameterParseError {
                name: format!("Command_{}", self.command_id),
                reason: format!(
                    "Payload length mismatch: declared {} bytes, but received {} bytes. Packet may have been truncated over network.",
                    self.payload_len,
                    self.payload.len()
                ),
            });
        }

        // 2. IEEE CRC32 Checksum verification (detects bit-flips or partial framing)
        let computed_crc = crc32fast::hash(&self.payload);
        if computed_crc != self.payload_crc32 {
            return Err(SterngateError::ChecksumMismatch {
                expected: format!("0x{:08X}", self.payload_crc32),
                calculated: format!("0x{:08X}", computed_crc),
            });
        }

        Ok(())
    }

    /// Freshness check: rejects commands held up in retransmission buffers
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        current_time_ms > self.timestamp_ms + self.ttl_ms
    }

    /// Schema validation: ensure parameters adhere to vehicle profile definitions
    pub fn validate_profile_schema(&self, profile: &VehicleProfile) -> Result<()> {
        if let Some(did) = self.did {
            let did_str = format!("0x{:04X}", did);
            if let Some(param) = profile.find_parameter(&did_str) {
                if param.module != self.target_module {
                    return Err(SterngateError::ProfileError(format!(
                        "DID 0x{:04X} belongs to module '{}', not '{}'",
                        did, param.module, self.target_module
                    )));
                }

                if self.payload.len() != param.length {
                    return Err(SterngateError::ParameterParseError {
                        name: param.name.clone(),
                        reason: format!(
                            "DID 0x{:04X} expects exactly {} bytes according to profile, but payload contains {} bytes.",
                            did, param.length, self.payload.len()
                        ),
                    });
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandValidationReport {
    pub is_valid: bool,
    pub command_id: String,
    pub length_verified: bool,
    pub crc32_verified: bool,
    pub freshness_verified: bool,
    pub schema_verified: bool,
    pub error: Option<String>,
}
