use std::time::Duration;
use sterngate_core::{CanFrame, Result, SterngateError};
use sterngate_hal::VehicleInterface;
use tokio::time::timeout;

fn pci_byte(frame: &CanFrame) -> Result<u8> {
    frame
        .data
        .first()
        .copied()
        .ok_or_else(|| SterngateError::IsoTpError("Empty CAN frame on ISO-TP channel".into()))
}

/// Maximum number of consecutive Flow Control WAIT frames tolerated (N_WFTmax).
const N_WFT_MAX: usize = 8;

/// Map an ISO-TP Flow Control STmin byte to a millisecond delay per ISO 15765-2:
/// 0x00-0x7F are 0-127 ms, 0xF1-0xF9 are 100-900 us (rounded up to 1 ms), and
/// every other value is reserved and falls back to a conservative default.
fn st_min_to_ms(st: u8) -> u64 {
    match st {
        st if st <= 127 => u64::from(st),
        st if (0xF1..=0xF9).contains(&st) => 1,
        _ => 10,
    }
}

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

    /// Change the receive timeout (used while the ECU reports NRC 0x78 ResponsePending).
    pub fn set_timeout(&mut self, duration: Duration) {
        self.timeout_duration = duration;
    }

    pub fn timeout(&self) -> Duration {
        self.timeout_duration
    }

    /// The 11-bit arbitration id every frame this channel sends goes out on.
    ///
    /// `tx_id` is caller-supplied (a `.sgmod` author controls
    /// `modpack.target.tx_id`), and this channel only builds standard frames.
    /// Casting a 29-bit id down to `u16` would silently address a *different*
    /// ECU, so an out-of-range id is refused instead of truncated.
    fn tx_can_id(&self) -> Result<u16> {
        u16::try_from(self.tx_id)
            .ok()
            .filter(|id| *id <= 0x7FF)
            .ok_or_else(|| {
                SterngateError::IsoTpError(format!(
                    "tx_id 0x{:X} is not an 11-bit standard CAN identifier (max 0x7FF); refusing to truncate it onto another ECU's address",
                    self.tx_id
                ))
            })
    }

    /// Send an ISO-TP payload (handles Single Frame and First Frame/Consecutive Frames)
    pub async fn send_payload(&mut self, payload: &[u8]) -> Result<()> {
        let tx_id = self.tx_can_id()?;
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
            let frame = CanFrame::new_standard(tx_id, &data);
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

        let ff_frame = CanFrame::new_standard(tx_id, &ff_data);
        self.interface.send(ff_frame).await?;

        // Wait for Flow Control (FC) frame from ECU
        let (mut block_size, mut st_min) = self.wait_for_flow_control().await?;
        let mut st_min_ms = st_min_to_ms(st_min);

        // Send Consecutive Frames (CF), honouring the Flow Control BlockSize:
        // when BS != 0, wait for a fresh Flow Control after every BS frames.
        let mut offset = 6;
        let mut seq_num = 1u8;
        let mut sent_in_block = 0u8;

        while offset < len {
            let chunk_size = (len - offset).min(7);
            let pci = 0x20 | (seq_num & 0x0F);
            let mut cf_data = vec![pci];
            cf_data.extend_from_slice(&payload[offset..offset + chunk_size]);
            while cf_data.len() < 8 {
                cf_data.push(0xAA);
            }

            let cf_frame = CanFrame::new_standard(tx_id, &cf_data);
            self.interface.send(cf_frame).await?;

            offset += chunk_size;
            seq_num = (seq_num + 1) % 16;

            if block_size != 0 && offset < len {
                sent_in_block += 1;
                if sent_in_block == block_size {
                    let (bs, st) = self.wait_for_flow_control().await?;
                    block_size = bs;
                    st_min = st;
                    st_min_ms = st_min_to_ms(st_min);
                    sent_in_block = 0;
                }
            }

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

        let pci = pci_byte(&frame)?;
        let pci_type = (pci >> 4) & 0x0F;
        match pci_type {
            // Single Frame (SF)
            0x00 => {
                let sf_len = usize::from(pci & 0x0F);
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
                if frame.data.len() < 8 {
                    return Err(SterngateError::IsoTpError(format!(
                        "First Frame too short: {} bytes",
                        frame.data.len()
                    )));
                }
                let total_len = (usize::from(pci & 0x0F) << 8) | usize::from(frame.data[1]);
                if total_len < 8 {
                    return Err(SterngateError::IsoTpError(format!(
                        "First Frame announces {total_len} bytes; a multi-frame message carries at least 8"
                    )));
                }
                let mut buffer = Vec::with_capacity(total_len);
                buffer.extend_from_slice(&frame.data[2..8]);

                // Send Flow Control frame: CTS (ContinueToSend = 0), BS = 0, STmin = 5ms
                let fc_frame = CanFrame::new_standard(
                    self.tx_can_id()?,
                    &[0x30, 0x00, 0x05, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA],
                );
                self.interface.send(fc_frame).await?;

                let mut expected_sn = 1u8;
                while buffer.len() < total_len {
                    let cf_frame = timeout(self.timeout_duration, self.wait_for_rx_frame())
                        .await
                        .map_err(|_| SterngateError::IsoTpTimeout)??;

                    let cf_pci_byte = pci_byte(&cf_frame)?;
                    let cf_pci = (cf_pci_byte >> 4) & 0x0F;
                    let sn = cf_pci_byte & 0x0F;
                    if cf_pci != 0x02 || sn != expected_sn {
                        return Err(SterngateError::IsoTpError(format!(
                            "Out-of-order CF: expected {}, got {}",
                            expected_sn, sn
                        )));
                    }
                    let remaining = total_len - buffer.len();
                    let available = cf_frame.data.len().saturating_sub(1);
                    let take_len = remaining.min(7).min(available);
                    if take_len == 0 {
                        return Err(SterngateError::IsoTpError(
                            "Truncated Consecutive Frame".into(),
                        ));
                    }
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

    /// Wait for one Flow Control frame and return `(block_size, st_min)`.
    ///
    /// Tolerates up to `N_WFT_MAX` consecutive WAIT (FS=1) frames, re-awaiting a
    /// fresh Flow Control each time; errors on OVFLW (FS=2) or any other status.
    async fn wait_for_flow_control(&mut self) -> Result<(u8, u8)> {
        for _ in 0..=N_WFT_MAX {
            let frame = timeout(self.timeout_duration, async {
                loop {
                    let f = self.interface.recv().await?;
                    if f.id == self.rx_id && f.data.first().is_some_and(|b| (b >> 4) == 0x03) {
                        return Ok::<CanFrame, SterngateError>(f);
                    }
                }
            })
            .await
            .map_err(|_| SterngateError::IsoTpTimeout)??;

            if frame.data.len() < 3 {
                return Err(SterngateError::IsoTpError(format!(
                    "Flow Control frame too short: {} bytes",
                    frame.data.len()
                )));
            }

            match frame.data[0] & 0x0F {
                0 => return Ok((frame.data[1], frame.data[2])),
                1 => continue, // WAIT: the receiver asks for another Flow Control
                2 => {
                    return Err(SterngateError::IsoTpError(
                        "Flow Control: receiver overflow".into(),
                    ))
                }
                fs => {
                    return Err(SterngateError::IsoTpError(format!(
                        "Flow Control status not CTS: {fs}"
                    )))
                }
            }
        }
        Err(SterngateError::IsoTpError(format!(
            "Flow Control WAIT repeated more than {N_WFT_MAX} times"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScriptedInterface;
    use std::time::Duration;

    /// `tx_id` comes from a `.sgmod` author (`modpack.target.tx_id`) or a
    /// profile, and this channel only builds 11-bit standard frames. Truncating
    /// a 29-bit id would send the request to a different ECU's address, so it
    /// is refused before a single frame goes on the bus.
    #[tokio::test(start_paused = true)]
    async fn extended_tx_id_is_refused_not_truncated() {
        for tx_id in [0x800, 0x1800_07E0, 0xFFFF_FFFF] {
            let mut iface = ScriptedInterface::new();
            let handle = iface.sent_handle();
            let mut ch = IsoTpChannel::new(&mut iface, tx_id, 0x7E8);
            // Single frame and multi-frame both refuse.
            for payload in [vec![0x22, 0xF1, 0x90], vec![0xAA; 32]] {
                let err = ch.send_payload(&payload).await.unwrap_err();
                assert!(
                    matches!(&err, SterngateError::IsoTpError(m) if m.contains("11-bit")),
                    "tx_id 0x{tx_id:X}: {err:?}"
                );
            }
            assert!(
                handle.lock().unwrap().is_empty(),
                "tx_id 0x{tx_id:X}: no frame may reach the bus"
            );
        }

        // 0x7FF is the last legal standard id and still sends.
        let mut iface = ScriptedInterface::new();
        let handle = iface.sent_handle();
        let mut ch = IsoTpChannel::new(&mut iface, 0x7FF, 0x7E8);
        ch.send_payload(&[0x22, 0xF1, 0x90]).await.unwrap();
        assert_eq!(handle.lock().unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn recv_payload_empty_frame_is_error_not_panic() {
        let mut iface = ScriptedInterface::new().raw_frames(&[&[]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(
            ch.recv_payload().await,
            Err(SterngateError::IsoTpError(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn first_frame_shorter_than_8_is_error() {
        let mut iface = ScriptedInterface::new().raw_frames(&[&[0x10, 0x0A, 0x62]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(
            ch.recv_payload().await,
            Err(SterngateError::IsoTpError(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn first_frame_announcing_less_than_8_is_error() {
        let mut iface = ScriptedInterface::new()
            .raw_frames(&[&[0x10, 0x05, 0x62, 0xF1, 0x92, 0x30, 0x31, 0x32]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(
            ch.recv_payload().await,
            Err(SterngateError::IsoTpError(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn consecutive_frame_short_is_error() {
        let mut iface = ScriptedInterface::new()
            .raw_frames(&[&[0x10, 0x0D, 0x62, 0xF1, 0x92, 0x30, 0x32, 0x38], &[0x21]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(
            ch.recv_payload().await,
            Err(SterngateError::IsoTpError(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn flow_control_short_is_error() {
        // 20-byte payload -> FF; the ECU answers with a 1-byte FC.
        let mut iface = ScriptedInterface::new().rule(0x2E, &[&[0x30]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        let payload = vec![0x2E; 20];
        assert!(matches!(
            ch.send_payload(&payload).await,
            Err(SterngateError::IsoTpError(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn multi_frame_reply_reassembles() {
        // 62 F1 92 + "0281012224" = 13 bytes
        let mut iface = ScriptedInterface::new().rule(
            0x22,
            &[
                &[0x10, 0x0D, 0x62, 0xF1, 0x92, b'0', b'2', b'8'],
                &[0x21, b'1', b'0', b'1', b'2', b'2', b'2', b'4'],
            ],
        );
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        ch.send_payload(&[0x22, 0xF1, 0x92]).await.unwrap();
        let resp = ch.recv_payload().await.unwrap();
        assert_eq!(&resp[3..], b"0281012224");
        // The receiver answered the FF with a Flow Control frame.
        assert!(iface
            .sent_frames()
            .iter()
            .any(|f| f.data.first() == Some(&0x30)));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_is_settable() {
        let mut iface = ScriptedInterface::new();
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert_eq!(ch.timeout(), Duration::from_millis(1500));
        ch.set_timeout(Duration::from_millis(50));
        assert_eq!(ch.timeout(), Duration::from_millis(50));
        assert!(matches!(
            ch.recv_payload().await,
            Err(SterngateError::IsoTpTimeout)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn sender_waits_for_fc_after_block_size() {
        // 20-byte payload = FF + 2 CFs. FC says BS = 1: after one CF the sender must wait for a second FC.
        let mut iface = ScriptedInterface::new().rule(0x2E, &[&[0x30, 0x01, 0x00]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        ch.set_timeout(Duration::from_millis(200));
        let payload = vec![0x2E; 20];
        // No second FC is scripted, so the sender must time out after exactly one CF.
        assert!(matches!(
            ch.send_payload(&payload).await,
            Err(SterngateError::IsoTpTimeout)
        ));
        let cfs = iface
            .sent_frames()
            .iter()
            .filter(|f| f.data.first().map(|b| b >> 4) == Some(2))
            .count();
        assert_eq!(cfs, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn flow_status_wait_is_honoured() {
        let mut iface =
            ScriptedInterface::new().rule(0x2E, &[&[0x31, 0x00, 0x00], &[0x30, 0x00, 0x00]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        ch.send_payload(&[0x2E; 20]).await.unwrap();
        let cfs = iface
            .sent_frames()
            .iter()
            .filter(|f| f.data.first().map(|b| b >> 4) == Some(2))
            .count();
        assert_eq!(cfs, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn flow_status_overflow_is_error() {
        let mut iface = ScriptedInterface::new().rule(0x2E, &[&[0x32, 0x00, 0x00]]);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        let err = ch.send_payload(&[0x2E; 20]).await.unwrap_err();
        assert!(err.to_string().contains("overflow"), "{err}");
    }

    #[tokio::test(start_paused = true)]
    async fn flow_status_wait_gives_up_after_n_wft_max() {
        let waits: Vec<&[u8]> = vec![&[0x31, 0, 0]; 9];
        let mut iface = ScriptedInterface::new().rule(0x2E, &waits);
        let mut ch = IsoTpChannel::new(&mut iface, 0x7E0, 0x7E8);
        assert!(matches!(
            ch.send_payload(&[0x2E; 20]).await,
            Err(SterngateError::IsoTpError(_))
        ));
    }
}
