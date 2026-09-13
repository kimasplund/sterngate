pub mod blobs;
pub mod node;
pub mod tunnel;

pub use blobs::StagedBlob;
pub use node::P2pNode;
pub use tunnel::P2pMessage;

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;
    use sterngate_core::FlashPackageManifest;

    #[test]
    fn test_staged_blob_verification() {
        let dummy_data = vec![0xCA, 0xFE, 0xBA, 0xBE];
        let mut hasher = sha2::Sha256::new();
        sha2::Digest::update(&mut hasher, &dummy_data);
        let sha256_checksum = format!("{:x}", sha2::Digest::finalize(hasher));
        let crc32_checksum = crc32fast::hash(&dummy_data);

        let manifest = FlashPackageManifest {
            target_module: "EDC16".into(),
            expected_hw_id: "0281012224".into(),
            expected_sw_id: "1037372332".into(),
            sha256_checksum: sha256_checksum.clone(),
            crc32_checksum,
            flash_start_address: 0,
            flash_length: 4,
            block_size: 256,
        };

        let blob = StagedBlob::new(manifest, dummy_data);
        assert!(blob.verify_integrity().is_ok());

        // Corrupt data
        let corrupted_blob = StagedBlob::new(blob.manifest.clone(), vec![0x00, 0x00, 0x00, 0x00]);
        assert!(corrupted_blob.verify_integrity().is_err());
    }

    #[test]
    fn test_ticket_encoding_decoding() {
        let fake_json = r#"{"id":"0000000000000000000000000000000000000000000000000000000000000000","addrs":[]}"#;
        let hex_str: String = fake_json
            .as_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        let ticket = format!("sterngate-ticket:{}", hex_str);

        let parsed = P2pNode::parse_ticket(&ticket);
        assert!(parsed.is_ok());
    }
}
