use std::time::Duration;
use sterngate_core::{CanFrame, Result, SterngateError};
use sterngate_hal::VehicleInterface;
use tokio::time::timeout;

pub struct IsoTpChannel<'a> {
    interface: &'a mut dyn VehicleInterface,
    tx_id: u32,
    rx_id: u32,
    timeout_duration: Duration,
}

impl<'a> IsoTpChannel<'a> {
    pub fn new(interface: &'a mut dyn VehicleInterface, tx_id: u32, rx_id: u32) -> Self {
        Self {
            interface,
            tx_id,
            rx_id,
            timeout_duration: Duration::from_millis(1500),
        }
    }

    pub fn with_timeout(mut self, duration: Duration) -> Self {
        self.timeout_duration = duration;
        self
    }

    /// Send an ISO-TP payload (handles Single Frame and First Frame/Consecutive Frames)
    pub async fn send_payload(&mut self, payload: &[u8]) -> Result<()> {
        let len = payload.len();
        if len == 0 {
            return Err(SterngateError::IsoTpError(
                "Cannot send empty ISO-TP payload".into(),
            ));
        }

        if len <= 7 {
            // Single Frame (SF)
            let mut data = vec![len as u8];
            data.extend_from_slice(payload);
            while data.len() < 8 {
                data.push(0xAA); // Padding
            }
            let frame = CanFrame::new_standard(self.tx_id as u16, &data);
            self.interface.send(frame).await?;
            return Ok(());
        }

        // Multi-frame: First Frame (FF)
        if len > 4095 {
            return Err(SterngateError::IsoTpError(format!(
                "Payload length {} exceeds ISO-TP 4095 limit",
                len
            )));
        }

        let ff_hi = 0x10 | ((len >> 8) as u8 & 0x0F);
        let ff_lo = (len & 0xFF) as u8;
        let mut ff_data = vec![ff_hi, ff_lo];
        ff_data.extend_from_slice(&payload[..6]);

        let ff_frame = CanFrame::new_standard(self.tx_id as u16, &ff_data);
        self.interface.send(ff_frame).await?;

        // Wait for Flow Control (FC) frame from ECU
        let fc = self.wait_for_flow_control().await?;
        let _block_size = fc[1];
        let st_min_ms = match fc[2] {
            st if st <= 127 => st as u64,
            st if (0xF1..=0xF9).contains(&st) => 1,
            _ => 10,
        };

        // Send Consecutive Frames (CF)
        let mut offset = 6;
        let mut seq_num = 1u8;

        while offset < len {
            let chunk_size = (len - offset).min(7);
            let pci = 0x20 | (seq_num & 0x0F);
            let mut cf_data = vec![pci];
            cf_data.extend_from_slice(&payload[offset..offset + chunk_size]);
            while cf_data.len() < 8 {
                cf_data.push(0xAA);
            }

            let cf_frame = CanFrame::new_standard(self.tx_id as u16, &cf_data);
            self.interface.send(cf_frame).await?;

            offset += chunk_size;
            seq_num = (seq_num + 1) % 16;

            if st_min_ms > 0 {
                tokio::time::sleep(Duration::from_millis(st_min_ms)).await;
            }
        }

        Ok(())
    }

    /// Receive an ISO-TP payload (handles SF and reassembly of multi-frame FF/CF)
    pub async fn recv_payload(&mut self) -> Result<Vec<u8>> {
        let frame = timeout(self.timeout_duration, self.wait_for_rx_frame())
            .await
            .map_err(|_| SterngateError::IsoTpTimeout)??;

        let pci_type = (frame.data[0] >> 4) & 0x0F;
        match pci_type {
            // Single Frame (SF)
            0x00 => {
                let sf_len = (frame.data[0] & 0x0F) as usize;
                if sf_len > 7 || sf_len == 0 {
                    return Err(SterngateError::IsoTpError(format!(
                        "Invalid SF DL: {}",
                        sf_len
                    )));
                }
                if frame.data.len() < 1 + sf_len {
                    return Err(SterngateError::IsoTpError("Truncated SF frame".into()));
                }
                Ok(frame.data[1..1 + sf_len].to_vec())
            }
            // First Frame (FF)
            0x01 => {
                let total_len = (((frame.data[0] & 0x0F) as usize) << 8) | (frame.data[1] as usize);
                let mut buffer = Vec::with_capacity(total_len);
                buffer.extend_from_slice(&frame.data[2..8]);

                // Send Flow Control frame: CTS (ContinueToSend = 0), BS = 0, STmin = 5ms
                let fc_frame = CanFrame::new_standard(
                    self.tx_id as u16,
                    &[0x30, 0x00, 0x05, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA],
                );
                self.interface.send(fc_frame).await?;

                let mut expected_sn = 1u8;
                while buffer.len() < total_len {
                    let cf_frame = timeout(self.timeout_duration, self.wait_for_rx_frame())
                        .await
                        .map_err(|_| SterngateError::IsoTpTimeout)??;

                    let cf_pci = (cf_frame.data[0] >> 4) & 0x0F;
                    let sn = cf_frame.data[0] & 0x0F;

                    if cf_pci != 0x02 || sn != expected_sn {
                        return Err(SterngateError::IsoTpError(format!(
                            "Out-of-order CF: expected {}, got {}",
                            expected_sn, sn
                        )));
                    }

                    let remaining = total_len - buffer.len();
                    let take_len = remaining.min(7);
                    buffer.extend_from_slice(&cf_frame.data[1..1 + take_len]);
                    expected_sn = (expected_sn + 1) % 16;
                }

                Ok(buffer)
            }
            _ => Err(SterngateError::IsoTpError(format!(
                "Unexpected initial frame PCI type: 0x{:X}",
                pci_type
            ))),
        }
    }

    async fn wait_for_rx_frame(&mut self) -> Result<CanFrame> {
        loop {
            let frame = self.interface.recv().await?;
            if frame.id == self.rx_id {
                return Ok(frame);
            }
        }
    }

    async fn wait_for_flow_control(&mut self) -> Result<Vec<u8>> {
        let frame = timeout(self.timeout_duration, async {
            loop {
                let f = self.interface.recv().await?;
                if f.id == self.rx_id && (f.data[0] >> 4) == 0x03 {
                    return Ok(f);
                }
            }
        })
        .await
        .map_err(|_| SterngateError::IsoTpTimeout)??;

        let flow_status = frame.data[0] & 0x0F;
        if flow_status != 0 {
            return Err(SterngateError::IsoTpError(format!(
                "Flow Control status not CTS: {}",
                flow_status
            )));
        }

        Ok(frame.data)
    }
}
