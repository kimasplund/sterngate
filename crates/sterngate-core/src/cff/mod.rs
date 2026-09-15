//! Mercedes SDflash `.CFF` flash containers. Phase 0 only recognises them so
//! the vault never treats a container as a raw image; Phase 1 adds the parser.

/// Byte-cheap detection of a CFF container: the ASCII prologue at offset 0
/// and the `0x05ED` stub magic at 0x400 (little-endian).
pub fn sniff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"CFF-TRANSLATOR-VERSION") && bytes.get(0x400..0x402) == Some(&[0xED, 0x05])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Minimal synthetic container: prologue, NUL padding, stub magic. No firmware bytes.
    pub(crate) fn synthetic_cff_header() -> Vec<u8> {
        let mut v = b"CFF-TRANSLATOR-VERSION:02.01.03\nDATE:15.9.2026\nFINGERPRINT:1.2.3.4\nCFF:TEST\nLANGUAGE:ORIGINAL\n".to_vec();
        v.resize(0x400, 0);
        v.extend_from_slice(&[0xED, 0x05, 0xEA, 0x07, 0x09, 0x0F]);
        v.resize(0x1000, 0xFF);
        v
    }

    #[test]
    fn sniff_recognises_prologue_and_stub_magic() {
        assert!(sniff(&synthetic_cff_header()));
        assert!(!sniff(b"CFF-TRANSLATOR-VERSION"));
        let mut wrong_magic = synthetic_cff_header();
        wrong_magic[0x400] = 0x00;
        assert!(!sniff(&wrong_magic));
        assert!(!sniff(&[0xEA; 4096]));
        assert!(!sniff(&[]));
    }
}
