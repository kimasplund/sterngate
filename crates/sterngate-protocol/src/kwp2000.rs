use crate::isotp::IsoTpChannel;
use sterngate_core::{Result, SterngateError};
use sterngate_hal::VehicleInterface;

pub struct KwpClient<'a> {
    channel: IsoTpChannel<'a>,
}

impl<'a> KwpClient<'a> {
    pub fn new(interface: &'a mut dyn VehicleInterface, tx_id: u32, rx_id: u32) -> Self {
        Self {
            channel: IsoTpChannel::new(interface, tx_id, rx_id),
        }
    }

    pub async fn send_request(&mut self, service: u8, payload: &[u8]) -> Result<Vec<u8>> {
        let mut req = vec![service];
        req.extend_from_slice(payload);
        self.channel.send_payload(&req).await?;

        let resp = self.channel.recv_payload().await?;
        if resp.is_empty() {
            return Err(SterngateError::IsoTpError("Empty KWP2000 response".into()));
        }

        if resp[0] == 0x7F {
            let rejected_service = if resp.len() > 1 { resp[1] } else { service };
            let nrc = if resp.len() > 2 { resp[2] } else { 0 };
            return Err(SterngateError::UdsNegativeResponse {
                service: rejected_service,
                nrc,
                description: format!("KWP2000 Reject 0x{:02X}", nrc),
            });
        }

        if resp[0] != service + 0x40 {
            return Err(SterngateError::IsoTpError(format!(
                "Unexpected KWP response SID: expected 0x{:02X}, got 0x{:02X}",
                service + 0x40,
                resp[0]
            )));
        }

        Ok(resp)
    }

    /// ReadDataByLocalIdentifier (0x21)
    pub async fn read_data_by_local_id(&mut self, record_id: u8) -> Result<Vec<u8>> {
        self.send_request(0x21, &[record_id]).await
    }

    /// WriteDataByLocalIdentifier (0x3B)
    pub async fn write_data_by_local_id(&mut self, record_id: u8, data: &[u8]) -> Result<Vec<u8>> {
        let mut payload = vec![record_id];
        payload.extend_from_slice(data);
        self.send_request(0x3B, &payload).await
    }
}
