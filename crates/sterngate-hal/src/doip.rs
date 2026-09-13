use crate::interface::VehicleInterface;
use async_trait::async_trait;
use sterngate_core::{CanFrame, Result, SterngateError};

#[derive(Debug)]
pub struct DoIpInterface {
    remote_addr: String,
    is_connected: bool,
}

impl DoIpInterface {
    pub fn new(remote_addr: &str) -> Self {
        Self {
            remote_addr: remote_addr.to_string(),
            is_connected: false,
        }
    }
}

#[async_trait]
impl VehicleInterface for DoIpInterface {
    async fn open(&mut self) -> Result<()> {
        tracing::info!("Connecting to DoIP gateway at {}", self.remote_addr);
        self.is_connected = true;
        Ok(())
    }

    async fn send(&mut self, frame: CanFrame) -> Result<()> {
        if !self.is_connected {
            return Err(SterngateError::DeviceNotFound(
                "DoIP session not connected".into(),
            ));
        }
        tracing::trace!("DoIP TX frame ID 0x{:X}", frame.id);
        Ok(())
    }

    async fn recv(&mut self) -> Result<CanFrame> {
        if !self.is_connected {
            return Err(SterngateError::DeviceNotFound(
                "DoIP session not connected".into(),
            ));
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        Ok(CanFrame::new_standard(0x7E8, &[0x02, 0x50, 0x01]))
    }

    async fn close(&mut self) -> Result<()> {
        self.is_connected = false;
        Ok(())
    }

    fn name(&self) -> &str {
        "DoIP (ISO 13400)"
    }

    fn is_connected(&self) -> bool {
        self.is_connected
    }
}
