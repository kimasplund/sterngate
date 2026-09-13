use sha2::{Digest, Sha256};
use sterngate_core::{FlashPackageManifest, SterngateError};

pub struct StagedBlob {
    pub manifest: FlashPackageManifest,
    pub data: Vec<u8>,
}

impl StagedBlob {
    pub fn new(manifest: FlashPackageManifest, data: Vec<u8>) -> Self {
        Self { manifest, data }
    }

    pub fn verify_integrity(&self) -> sterngate_core::Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(&self.data);
        let calculated = format!("{:x}", hasher.finalize());

        if !calculated.eq_ignore_ascii_case(&self.manifest.sha256_checksum) {
            return Err(SterngateError::ChecksumMismatch {
                expected: self.manifest.sha256_checksum.clone(),
                calculated,
            });
        }

        let calc_crc = crc32fast::hash(&self.data);
        if calc_crc != self.manifest.crc32_checksum {
            return Err(SterngateError::ChecksumMismatch {
                expected: format!("0x{:08X}", self.manifest.crc32_checksum),
                calculated: format!("0x{:08X}", calc_crc),
            });
        }

        Ok(())
    }
}
