use crate::interface::VehicleInterface;
use async_trait::async_trait;
use socketcan::{tokio::CanSocket, CanDataFrame, EmbeddedFrame, ExtendedId, Id, StandardId};
use sterngate_core::{CanFrame, Result, SterngateError};

#[derive(Debug)]
pub struct SocketCanInterface {
    interface_name: String,
    socket: Option<CanSocket>,
}

impl SocketCanInterface {
    pub fn new(interface_name: &str) -> Self {
        Self {
            interface_name: interface_name.to_string(),
            socket: None,
        }
    }
}

#[async_trait]
impl VehicleInterface for SocketCanInterface {
    async fn open(&mut self) -> Result<()> {
        let sock = CanSocket::open(&self.interface_name).map_err(|e| {
            SterngateError::HalError(format!(
                "Failed to open SocketCAN interface '{}': {}",
                self.interface_name, e
            ))
        })?;
        self.socket = Some(sock);
        Ok(())
    }

    async fn send(&mut self, frame: CanFrame) -> Result<()> {
        let sock = self.socket.as_mut().ok_or_else(|| {
            SterngateError::DeviceNotFound(format!(
                "SocketCAN '{}' is not open",
                self.interface_name
            ))
        })?;

        let can_id = if frame.is_extended {
            Id::Extended(ExtendedId::new(frame.id).ok_or_else(|| {
                SterngateError::HalError(format!("Invalid extended CAN ID: 0x{:X}", frame.id))
            })?)
        } else {
            Id::Standard(StandardId::new(frame.id as u16).ok_or_else(|| {
                SterngateError::HalError(format!("Invalid standard CAN ID: 0x{:X}", frame.id))
            })?)
        };

        let sk_frame = CanDataFrame::new(can_id, &frame.data).ok_or_else(|| {
            SterngateError::HalError("Failed to build SocketCAN data frame".into())
        })?;

        sock.write_frame(socketcan::CanFrame::Data(sk_frame))
            .await
            .map_err(|e| {
                SterngateError::HalError(format!("Failed to write SocketCAN frame: {}", e))
            })?;

        Ok(())
    }

    async fn recv(&mut self) -> Result<CanFrame> {
        let sock = self.socket.as_mut().ok_or_else(|| {
            SterngateError::DeviceNotFound(format!(
                "SocketCAN '{}' is not open",
                self.interface_name
            ))
        })?;

        let frame = sock.read_frame().await.map_err(|e| {
            SterngateError::HalError(format!("Failed to read SocketCAN frame: {}", e))
        })?;

        let id = match frame.id() {
            Id::Standard(sid) => sid.as_raw() as u32,
            Id::Extended(eid) => eid.as_raw(),
        };

        let is_extended = matches!(frame.id(), Id::Extended(_));
        let data = frame.data().to_vec();

        Ok(CanFrame {
            id,
            is_extended,
            is_remote: frame.is_remote_frame(),
            dlc: data.len() as u8,
            data,
            timestamp_us: 0,
        })
    }

    async fn close(&mut self) -> Result<()> {
        self.socket = None;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.interface_name
    }

    fn is_connected(&self) -> bool {
        self.socket.is_some()
    }
}
