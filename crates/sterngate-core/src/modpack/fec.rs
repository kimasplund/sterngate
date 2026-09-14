//! Reed-Solomon Forward Error Correction (FEC) over GF(2^8)
//!
//! Provides deterministic error detection and automated in-place repair for
//! automotive mod packages subject to transmission corruption, copy-paste mangling,
//! and dropped or flipped bytes.
//!
//! Parameters:
//! - Field: GF(2^8) with primitive polynomial p(x) = x^8 + x^4 + x^3 + x^2 + 1 (0x11D)
//! - Generator alpha = 2
//! - Default Block Size: K = 239 data bytes, 2t = 16 parity bytes (N = 255 total bytes)
//! - Error Correction Capacity: t = 8 corrupted/altered bytes per block

use serde::{Deserialize, Serialize};

/// Primitive polynomial for GF(2^8): x^8 + x^4 + x^3 + x^2 + 1
const PRIMITIVE_POLY: u16 = 0x11D;
pub const DEFAULT_DATA_BLOCK_LEN: usize = 239;
pub const DEFAULT_PARITY_LEN: usize = 16; // 2t = 16 => t = 8 correctable errors

/// GF(2^8) finite field tables
pub struct GaloisField {
    exp: [u8; 512],
    log: [u8; 256],
}

impl Default for GaloisField {
    fn default() -> Self {
        Self::new()
    }
}

impl GaloisField {
    pub const fn new() -> Self {
        let mut exp = [0u8; 512];
        let mut log = [0u8; 256];

        let mut x: u16 = 1;
        let mut i = 0;
        while i < 255 {
            exp[i] = x as u8;
            exp[i + 255] = x as u8;
            log[x as usize] = i as u8;
            x <<= 1;
            if (x & 0x100) != 0 {
                x ^= PRIMITIVE_POLY;
            }
            i += 1;
        }

        Self { exp, log }
    }

    #[inline]
    pub fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            0
        } else {
            let idx = (self.log[a as usize] as usize) + (self.log[b as usize] as usize);
            self.exp[idx]
        }
    }

    #[inline]
    pub fn div(&self, a: u8, b: u8) -> u8 {
        assert!(b != 0, "Division by zero in GF(2^8)");
        if a == 0 {
            0
        } else {
            let log_a = self.log[a as usize] as isize;
            let log_b = self.log[b as usize] as isize;
            let mut diff = log_a - log_b;
            if diff < 0 {
                diff += 255;
            }
            self.exp[diff as usize]
        }
    }

    #[inline]
    pub fn inv(&self, a: u8) -> u8 {
        self.div(1, a)
    }

    #[inline]
    pub fn poly_eval(&self, poly: &[u8], x: u8) -> u8 {
        if poly.is_empty() {
            return 0;
        }
        let mut y = poly[0];
        for &coeff in &poly[1..] {
            y = self.mul(y, x) ^ coeff;
        }
        y
    }
}

pub static GF: GaloisField = GaloisField::new();

/// Build Reed-Solomon generator polynomial for given parity length 2t:
/// g(x) = (x - alpha^0)(x - alpha^1)...(x - alpha^(2t-1))
pub fn rs_generator_poly(parity_len: usize) -> Vec<u8> {
    let mut g = vec![1u8];
    for i in 0..parity_len {
        let root = GF.exp[i];
        let mut next_g = vec![0u8; g.len() + 1];
        for j in 0..g.len() {
            next_g[j] ^= g[j];
            next_g[j + 1] ^= GF.mul(g[j], root);
        }
        g = next_g;
    }
    g
}

/// Result of Reed-Solomon FEC inspection and repair
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FecStatus {
    /// Payload is perfectly intact with 0 errors
    Intact,
    /// Corrupted bytes were detected and successfully repaired in-place
    Repaired {
        corrected_byte_count: usize,
        repaired_offsets: Vec<usize>,
    },
    /// Damage exceeds error correction capacity (> t errors or truncated)
    Unrecoverable { reason: String },
}

/// Reed-Solomon FEC Codec for data blocks
pub struct ReedSolomonCodec {
    data_block_len: usize,
    parity_len: usize,
    generator: Vec<u8>,
}

impl ReedSolomonCodec {
    pub fn new(data_block_len: usize, parity_len: usize) -> Self {
        assert!(
            data_block_len + parity_len <= 255,
            "N = data + parity must be <= 255"
        );
        let generator = rs_generator_poly(parity_len);
        Self {
            data_block_len,
            parity_len,
            generator,
        }
    }

    pub fn default_codec() -> Self {
        Self::new(DEFAULT_DATA_BLOCK_LEN, DEFAULT_PARITY_LEN)
    }

    /// Generate parity symbols for arbitrary-length message bytes.
    /// Splits message into blocks of `data_block_len` and returns concatenated parity bytes.
    pub fn encode(&self, message: &[u8]) -> Vec<u8> {
        let mut all_parity = Vec::new();
        for chunk in message.chunks(self.data_block_len) {
            let mut padded_chunk = chunk.to_vec();
            padded_chunk.resize(self.data_block_len, 0);

            let parity = self.encode_block(&padded_chunk);
            all_parity.extend_from_slice(&parity);
        }
        all_parity
    }

    /// Encode a single data block of length `data_block_len` to generate `parity_len` bytes
    pub fn encode_block(&self, data: &[u8]) -> Vec<u8> {
        assert_eq!(data.len(), self.data_block_len);
        let mut remainder = vec![0u8; self.parity_len];

        for &byte in data {
            let feedback = byte ^ remainder[0];
            remainder.copy_within(1..self.parity_len, 0);
            remainder[self.parity_len - 1] = 0;

            if feedback != 0 {
                for (rem, &gen) in remainder.iter_mut().zip(&self.generator[1..]) {
                    *rem ^= GF.mul(gen, feedback);
                }
            }
        }

        remainder
    }

    /// Verify and repair corrupted data in-place using parity bytes.
    /// Returns `FecStatus` detailing whether the data was intact, repaired, or unrecoverable.
    pub fn decode_and_repair(&self, data: &mut [u8], parity: &[u8]) -> FecStatus {
        let expected_parity_len = data.chunks(self.data_block_len).count() * self.parity_len;
        if parity.len() < expected_parity_len {
            return FecStatus::Unrecoverable {
                reason: format!(
                    "Parity buffer truncated: expected {} bytes, received {} bytes",
                    expected_parity_len,
                    parity.len()
                ),
            };
        }

        let mut total_repaired = 0;
        let mut repaired_offsets = Vec::new();

        let mut data_offset = 0;
        let mut parity_offset = 0;

        while data_offset < data.len() {
            let chunk_len = (data.len() - data_offset).min(self.data_block_len);
            let mut block = vec![0u8; self.data_block_len + self.parity_len];
            block[..chunk_len].copy_from_slice(&data[data_offset..data_offset + chunk_len]);

            let chunk_parity = &parity[parity_offset..parity_offset + self.parity_len];
            block[self.data_block_len..].copy_from_slice(chunk_parity);

            // Compute syndromes
            let syndromes = self.compute_syndromes(&block);
            let has_errors = syndromes.iter().any(|&s| s != 0);

            if has_errors {
                // Attempt repair via Berlekamp-Massey & Chien Search
                match self.repair_block(&mut block, &syndromes) {
                    Ok(corrected_indices) => {
                        for idx in corrected_indices {
                            if idx < chunk_len {
                                data[data_offset + idx] = block[idx];
                                repaired_offsets.push(data_offset + idx);
                                total_repaired += 1;
                            }
                        }
                    }
                    Err(e) => {
                        return FecStatus::Unrecoverable {
                            reason: format!("Unrecoverable corruption in block: {}", e),
                        };
                    }
                }
            }

            data_offset += chunk_len;
            parity_offset += self.parity_len;
        }

        if total_repaired > 0 {
            FecStatus::Repaired {
                corrected_byte_count: total_repaired,
                repaired_offsets,
            }
        } else {
            FecStatus::Intact
        }
    }

    fn compute_syndromes(&self, block: &[u8]) -> Vec<u8> {
        let mut syndromes = vec![0u8; self.parity_len];
        for (i, syn) in syndromes.iter_mut().enumerate().take(self.parity_len) {
            let root = GF.exp[i];
            *syn = GF.poly_eval(block, root);
        }
        syndromes
    }

    fn repair_block(&self, block: &mut [u8], syndromes: &[u8]) -> Result<Vec<usize>, String> {
        let max_errors = self.parity_len / 2;

        // 1. Berlekamp-Massey algorithm to find Error Locator Polynomial Lambda(x)
        let mut lambda = vec![1u8];
        let mut b = vec![1u8];
        let mut l = 0usize;
        let mut k = 1usize;

        for n in 0..self.parity_len {
            let mut delta = syndromes[n];
            for j in 1..=l {
                if j < lambda.len() && n >= j {
                    delta ^= GF.mul(lambda[j], syndromes[n - j]);
                }
            }

            if delta != 0 {
                let temp_lambda = lambda.clone();
                let factor = delta;

                // lambda = lambda - delta * b * x^k
                let mut shifted_b = vec![0u8; k];
                shifted_b.extend_from_slice(&b);

                while lambda.len() < shifted_b.len() {
                    lambda.push(0);
                }

                for (idx, &coeff) in shifted_b.iter().enumerate() {
                    lambda[idx] ^= GF.mul(factor, coeff);
                }

                if 2 * l <= n {
                    let inv_delta = GF.inv(delta);
                    b = temp_lambda
                        .into_iter()
                        .map(|c| GF.mul(c, inv_delta))
                        .collect();
                    l = n + 1 - l;
                    k = 1;
                } else {
                    k += 1;
                }
            } else {
                k += 1;
            }
        }

        if l > max_errors {
            return Err(format!(
                "Too many errors detected ({} > {} capacity)",
                l, max_errors
            ));
        }

        // 2. Chien Search to find roots of Lambda(x)
        let block_len = block.len();
        let mut error_positions = Vec::new();

        for i in 0..block_len {
            let exp_val = (255 - (block_len - 1 - i) % 255) % 255;
            let x_inv = GF.exp[exp_val];

            let mut sum = 0u8;
            let mut x_pow = 1u8;
            for &coeff in &lambda {
                sum ^= GF.mul(coeff, x_pow);
                x_pow = GF.mul(x_pow, x_inv);
            }

            if sum == 0 {
                error_positions.push(i);
            }
        }

        if error_positions.len() != l {
            return Err(format!(
                "Chien search found {} error locations but Lambda degree was {}",
                error_positions.len(),
                l
            ));
        }

        // 3. Forney's Algorithm for Error Evaluator Polynomial Omega(x)
        // Omega(x) = S(x) * Lambda(x) mod x^(2t)
        let mut omega = vec![0u8; self.parity_len];
        for i in 0..self.parity_len {
            for j in 0..lambda.len() {
                if i + j < self.parity_len {
                    omega[i + j] ^= GF.mul(syndromes[i], lambda[j]);
                }
            }
        }

        // Compute error values and repair
        for &pos in &error_positions {
            let exp_val = (255 - (block_len - 1 - pos) % 255) % 255;
            let x_inv = GF.exp[exp_val];

            // Evaluate Omega(X_inv)
            let mut num = 0u8;
            let mut x_pow = 1u8;
            for &coeff in &omega {
                num ^= GF.mul(coeff, x_pow);
                x_pow = GF.mul(x_pow, x_inv);
            }

            // Formal derivative of Lambda: Lambda'(X_inv)
            let mut den = 0u8;
            let mut x_pow = 1u8;
            for (j, &coeff) in lambda.iter().enumerate().skip(1) {
                if j % 2 == 1 {
                    den ^= GF.mul(coeff, x_pow);
                }
                x_pow = GF.mul(x_pow, x_inv);
            }

            if den == 0 {
                return Err("Derivative vanished at error root in Forney algorithm".into());
            }

            let x_k = GF.exp[(block_len - 1 - pos) % 255];
            let error_val = GF.mul(GF.div(num, den), x_k);
            block[pos] ^= error_val;
        }

        Ok(error_positions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gf256_arithmetic() {
        assert_eq!(GF.mul(0, 55), 0);
        assert_eq!(GF.mul(1, 42), 42);
        assert_eq!(GF.mul(2, 2), 4);
        assert_eq!(GF.inv(1), 1);
        let a = 173;
        let inv_a = GF.inv(a);
        assert_eq!(GF.mul(a, inv_a), 1);
        assert_eq!(GF.div(50, 50), 1);
    }

    #[test]
    fn test_rs_codec_clean_payload() {
        let codec = ReedSolomonCodec::default_codec();
        let mut msg = b"W211 OM642 Throttle Sharpening and Agility Coding Payload".to_vec();
        let parity = codec.encode(&msg);

        let status = codec.decode_and_repair(&mut msg, &parity);
        assert_eq!(status, FecStatus::Intact);
    }

    #[test]
    fn test_rs_codec_single_and_multi_byte_repair() {
        let codec = ReedSolomonCodec::default_codec();
        let original_msg = b"Engine speed limiter VMax=250km/h DID 0x0110 EDC16CP31 OM642".to_vec();
        let parity = codec.encode(&original_msg);

        // Corrupt 3 random bytes
        let mut corrupted_msg = original_msg.clone();
        corrupted_msg[5] ^= 0x55;
        corrupted_msg[18] ^= 0xAA;
        corrupted_msg[32] ^= 0x0F;

        let status = codec.decode_and_repair(&mut corrupted_msg, &parity);
        match status {
            FecStatus::Repaired {
                corrected_byte_count,
                repaired_offsets,
            } => {
                assert_eq!(corrected_byte_count, 3);
                assert_eq!(repaired_offsets, vec![5, 18, 32]);
                assert_eq!(corrupted_msg, original_msg);
            }
            other => panic!("Expected Repaired, got {:?}", other),
        }
    }

    #[test]
    fn test_rs_codec_max_capacity_repair_8_errors() {
        let codec = ReedSolomonCodec::default_codec();
        let mut original_msg = vec![0x42; 200];
        for (i, b) in original_msg.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }
        let parity = codec.encode(&original_msg);

        // Corrupt up to 8 bytes (max capacity for 2t = 16 parity bytes)
        let mut corrupted_msg = original_msg.clone();
        for &offset in &[2, 15, 40, 75, 110, 145, 180, 195] {
            corrupted_msg[offset] ^= 0x7E;
        }

        let status = codec.decode_and_repair(&mut corrupted_msg, &parity);
        match status {
            FecStatus::Repaired {
                corrected_byte_count,
                ..
            } => {
                assert_eq!(corrected_byte_count, 8);
                assert_eq!(corrupted_msg, original_msg);
            }
            other => panic!("Expected Repaired for 8 errors, got {:?}", other),
        }
    }

    #[test]
    fn test_rs_codec_unrecoverable_exceeds_capacity() {
        let codec = ReedSolomonCodec::default_codec();
        let original_msg = vec![0x10; 100];
        let parity = codec.encode(&original_msg);

        // Corrupt 10 bytes (> 8)
        let mut corrupted_msg = original_msg.clone();
        for i in 0..10 {
            corrupted_msg[i * 8] ^= 0xFF;
        }

        let status = codec.decode_and_repair(&mut corrupted_msg, &parity);
        assert!(matches!(status, FecStatus::Unrecoverable { .. }));
    }
}
