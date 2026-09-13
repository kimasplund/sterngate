use std::collections::HashSet;
use std::sync::Arc;
use sterngate_core::{
    CommandEnvelope, CommandValidationReport, Result, SterngateError, VehicleProfile,
};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct TransactionGate {
    processed_idempotency_keys: Arc<Mutex<HashSet<String>>>,
    max_cache_size: usize,
}

impl TransactionGate {
    pub fn new() -> Self {
        Self {
            processed_idempotency_keys: Arc::new(Mutex::new(HashSet::new())),
            max_cache_size: 10_000,
        }
    }

    /// Verifies command integrity, freshness, deduplication, and profile schema
    pub async fn verify_and_authorize(
        &self,
        envelope: &CommandEnvelope,
        profile: &VehicleProfile,
    ) -> Result<CommandValidationReport> {
        // 1. Length & CRC32 integrity check
        envelope.validate_integrity()?;

        // 2. Freshness check (reject stale retransmissions beyond TTL)
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        if envelope.is_expired(now_ms) {
            return Err(SterngateError::PreFlightCheckFailed(format!(
                "Command {} expired: timestamp was {}ms ago, exceeding TTL of {}ms",
                envelope.command_id,
                now_ms.saturating_sub(envelope.timestamp_ms),
                envelope.ttl_ms
            )));
        }

        // 3. Idempotency deduplication check
        let mut keys = self.processed_idempotency_keys.lock().await;
        if keys.contains(&envelope.idempotency_key) {
            return Err(SterngateError::PreFlightCheckFailed(format!(
                "Duplicate command detected: idempotency key '{}' already processed",
                envelope.idempotency_key
            )));
        }

        // 4. Schema check against loaded profile
        envelope.validate_profile_schema(profile)?;

        // Register idempotency key
        if keys.len() >= self.max_cache_size {
            keys.clear(); // Simple eviction to bound memory
        }
        keys.insert(envelope.idempotency_key.clone());

        Ok(CommandValidationReport {
            is_valid: true,
            command_id: envelope.command_id.clone(),
            length_verified: true,
            crc32_verified: true,
            freshness_verified: true,
            schema_verified: true,
            error: None,
        })
    }
}

impl Default for TransactionGate {
    fn default() -> Self {
        Self::new()
    }
}
