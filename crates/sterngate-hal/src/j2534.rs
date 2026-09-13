use crate::interface::VehicleInterface;
use async_trait::async_trait;
use sterngate_core::{CanFrame, Result, SterngateError};

#[derive(Debug)]
pub struct J2534Interface {
    device_name: String,
    channel_id: Option<u32>,
    is_open: bool,
}

impl J2534Interface {
    pub fn new(device_name: &str) -> Self {
        Self {
            device_name: device_name.to_string(),
            channel_id: None,
            is_open: false,
        }
    }
}

#[async_trait]
impl VehicleInterface for J2534Interface {
    async fn open(&mut self) -> Result<()> {
        tracing::info!("Initializing J2534 PassThru device: {}", self.device_name);
        self.channel_id = Some(1);
        self.is_open = true;
        Ok(())
    }

    async fn send(&mut self, frame: CanFrame) -> Result<()> {
        if !self.is_open {
            return Err(SterngateError::DeviceNotFound(
                "J2534 device not open".into(),
            ));
        }
        tracing::trace!("J2534 TX: ID=0x{:X}, Len={}", frame.id, frame.data.len());
        Ok(())
    }

    async fn recv(&mut self) -> Result<CanFrame> {
        if !self.is_open {
            return Err(SterngateError::DeviceNotFound(
                "J2534 device not open".into(),
            ));
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Ok(CanFrame::new_standard(0x7E8, &[0x03, 0x7F, 0x22, 0x11]))
    }

    async fn close(&mut self) -> Result<()> {
        self.is_open = false;
        self.channel_id = None;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.device_name
    }

    fn is_connected(&self) -> bool {
        self.is_open
    }
}
