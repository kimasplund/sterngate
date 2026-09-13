use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanFrame {
    pub id: u32,
    pub is_extended: bool,
    pub is_remote: bool,
    pub dlc: u8,
    pub data: Vec<u8>,
    pub timestamp_us: u64,
}

impl CanFrame {
    pub fn new_standard(id: u16, data: &[u8]) -> Self {
        let len = data.len().min(8);
        Self {
            id: id as u32,
            is_extended: false,
            is_remote: false,
            dlc: len as u8,
            data: data[..len].to_vec(),
            timestamp_us: 0,
        }
    }

    pub fn new_extended(id: u32, data: &[u8]) -> Self {
        let len = data.len().min(8);
        Self {
            id,
            is_extended: true,
            is_remote: false,
            dlc: len as u8,
            data: data[..len].to_vec(),
            timestamp_us: 0,
        }
    }

    pub fn with_timestamp(mut self, timestamp_us: u64) -> Self {
        self.timestamp_us = timestamp_us;
        self
    }
}
