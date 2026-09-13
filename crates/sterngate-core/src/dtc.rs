use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dtc {
    pub code: String,
    pub raw_bytes: [u8; 3],
    pub status_byte: u8,
    pub module: String,
    pub description: String,
    pub confirmed: bool,
    pub pending: bool,
    pub warning_lamp_requested: bool,
}

use crate::i18n::{lookup_dtc_description, Language};

impl Dtc {
    pub fn parse_iso15031(b0: u8, b1: u8, status_byte: u8, module: &str) -> Self {
        Self::parse_iso15031_localized(b0, b1, status_byte, module, Language::En)
    }

    pub fn parse_iso15031_localized(
        b0: u8,
        b1: u8,
        status_byte: u8,
        module: &str,
        lang: Language,
    ) -> Self {
        let prefix = match (b0 >> 6) & 0x03 {
            0 => 'P',
            1 => 'C',
            2 => 'B',
            3 => 'U',
            _ => 'P',
        };
        let d1 = (b0 >> 4) & 0x03;
        let d2 = b0 & 0x0F;
        let d3 = (b1 >> 4) & 0x0F;
        let d4 = b1 & 0x0F;
        let code = format!("{}{:X}{:X}{:X}{:X}", prefix, d1, d2, d3, d4);

        let confirmed = (status_byte & 0x08) != 0;
        let pending = (status_byte & 0x04) != 0;
        let warning_lamp_requested = (status_byte & 0x80) != 0;

        let description = lookup_dtc_description(&code, lang);

        Self {
            code,
            raw_bytes: [b0, b1, 0],
            status_byte,
            module: module.to_string(),
            description,
            confirmed,
            pending,
            warning_lamp_requested,
        }
    }

    pub fn localize(&mut self, lang: Language) {
        self.description = lookup_dtc_description(&self.code, lang);
    }
}
