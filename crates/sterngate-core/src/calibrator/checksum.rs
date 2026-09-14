use crate::error::Result;
use serde::{Deserialize, Serialize};

/// Represents a single Bosch MPC5xx / 29BL802C flash checksum block
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksumBlock {
    pub block_index: usize,
    pub start_address: u32,
    pub end_address: u32,
    pub sum_address: u32,
    pub inv_address: u32,
    pub stored_sum: u32,
    pub calculated_sum: u32,
    pub stored_inv: u32,
    pub calculated_inv: u32,
    pub is_valid: bool,
}

/// Report detailing the integrity of all checksum blocks in the ROM
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksumReport {
    pub is_valid: bool,
    pub total_blocks: usize,
    pub valid_blocks: usize,
    pub blocks: Vec<ChecksumBlock>,
    pub global_crc32: u32,
}

pub struct BoschChecksumSolver;

impl BoschChecksumSolver {
    /// Scan and verify all Bosch checksum blocks in the ROM binary
    pub fn verify(rom: &[u8]) -> ChecksumReport {
        let blocks = Self::find_and_compute_blocks(rom);
        let total_blocks = blocks.len();
        let valid_blocks = blocks.iter().filter(|b| b.is_valid).count();
        let is_valid = total_blocks > 0 && valid_blocks == total_blocks;
        let global_crc32 = crc32fast::hash(rom);

        ChecksumReport {
            is_valid,
            total_blocks,
            valid_blocks,
            blocks,
            global_crc32,
        }
    }

    /// Recalculate all checksum blocks and patch them directly into the mutable ROM buffer
    pub fn recalculate_and_apply(rom: &mut [u8]) -> Result<ChecksumReport> {
        let mut report = Self::verify(rom);

        for block in &mut report.blocks {
            let sum_offset = block.sum_address as usize;
            let inv_offset = block.inv_address as usize;

            if sum_offset + 4 <= rom.len() && inv_offset + 4 <= rom.len() {
                rom[sum_offset..sum_offset + 4]
                    .copy_from_slice(&block.calculated_sum.to_be_bytes());
                rom[inv_offset..inv_offset + 4]
                    .copy_from_slice(&block.calculated_inv.to_be_bytes());
                block.stored_sum = block.calculated_sum;
                block.stored_inv = block.calculated_inv;
                block.is_valid = true;
            }
        }

        report.valid_blocks = report.blocks.len();
        report.is_valid = !report.blocks.is_empty();
        report.global_crc32 = crc32fast::hash(rom);

        Ok(report)
    }

    /// Locate Bosch block boundaries and calculate 32-bit additive sums
    fn find_and_compute_blocks(rom: &[u8]) -> Vec<ChecksumBlock> {
        let len = rom.len();
        if len < 0x80000 {
            // Less than 512KB, return empty
            return Vec::new();
        }

        // Standard Bosch EDC16 memory partitioning (2MB Flash = 0x200000):
        // Block 0: Bootloader & MPC config (0x000000 - 0x03FFFF)
        // Block 1: OS / Low-level stack (0x040000 - 0x07FFFF)
        // Block 2: Code / Main engine routines (0x080000 - 0x1BFFFF)
        // Block 3: Calibration data & Maps (0x1C0000 - 0x1FFFFF)
        let partitions: &[(u32, u32, u32, u32)] = if len >= 0x200000 {
            &[
                (0x000000, 0x03FFDF, 0x03FFE0, 0x03FFE4),
                (0x040000, 0x07FFDF, 0x07FFE0, 0x07FFE4),
                (0x080000, 0x1BFFDF, 0x1BFFE0, 0x1BFFE4),
                (0x1C0000, 0x1FFFDF, 0x1FFFE0, 0x1FFFE4),
            ]
        } else if len >= 0x100000 {
            // 1MB Flash
            &[
                (0x000000, 0x03FFDF, 0x03FFE0, 0x03FFE4),
                (0x040000, 0x07FFDF, 0x07FFE0, 0x07FFE4),
                (0x080000, 0x0DFFDF, 0x0DFFE0, 0x0DFFE4),
                (0x0E0000, 0x0FFFDF, 0x0FFFE0, 0x0FFFE4),
            ]
        } else {
            return Vec::new();
        };

        let mut blocks = Vec::new();

        for (idx, &(start, end, sum_addr, inv_addr)) in partitions.iter().enumerate() {
            let start_usize = start as usize;
            let end_usize = (end as usize).min(len);
            let sum_usize = sum_addr as usize;
            let inv_usize = inv_addr as usize;

            if sum_usize + 4 > len || inv_usize + 4 > len || start_usize >= end_usize {
                continue;
            }

            let stored_sum =
                u32::from_be_bytes(rom[sum_usize..sum_usize + 4].try_into().unwrap_or([0; 4]));
            let stored_inv =
                u32::from_be_bytes(rom[inv_usize..inv_usize + 4].try_into().unwrap_or([0; 4]));

            // Compute 32-bit big-endian sum across block, skipping sum & inv locations
            let mut sum: u32 = 0;
            let mut ptr = start_usize;
            while ptr + 4 <= end_usize {
                if ptr != sum_usize && ptr != inv_usize {
                    let word = u32::from_be_bytes(rom[ptr..ptr + 4].try_into().unwrap_or([0; 4]));
                    sum = sum.wrapping_add(word);
                }
                ptr += 4;
            }

            let inv = sum ^ 0xFFFF_FFFF;
            let is_valid = (stored_sum == sum && stored_inv == inv)
                || (stored_sum == 0 && stored_inv == 0 && sum == 0);

            blocks.push(ChecksumBlock {
                block_index: idx,
                start_address: start,
                end_address: end,
                sum_address: sum_addr,
                inv_address: inv_addr,
                stored_sum,
                calculated_sum: sum,
                stored_inv,
                calculated_inv: inv,
                is_valid,
            });
        }

        blocks
    }
}
