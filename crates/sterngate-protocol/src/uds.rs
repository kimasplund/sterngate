use crate::isotp::IsoTpChannel;
use crate::seedkey::SeedKeySolver;
use std::ops::{Deref, DerefMut};
use std::time::Duration;
use sterngate_core::{Dtc, Result, SterngateError};
use sterngate_hal::VehicleInterface;

/// Extra wait beyond the ECU-announced P2* before giving up on a pending reply.
const P2_STAR_MARGIN: Duration = Duration::from_millis(500);
/// P2* used until a DiagnosticSessionControl reply announces the real value.
const DEFAULT_P2_STAR: Duration = Duration::from_millis(5000);

/// P2* (enhanced response timing) from a positive DiagnosticSessionControl
/// reply: bytes 4..6 hold the value in 10 ms units. Widened before multiplying
/// so 0xFFFF cannot overflow a u16.
pub(crate) fn p2_star_from(resp: &[u8]) -> Option<Duration> {
    let raw = u16::from_be_bytes([*resp.get(4)?, *resp.get(5)?]);
    Some(Duration::from_millis(u64::from(raw) * 10))
}

/// Restores a channel's original timeout when dropped, including when the
/// enclosing future is cancelled mid-await (a caller wrapping the request in
/// `tokio::time::timeout`/`select!`, or dropped on client disconnect) while
/// NRC 0x78 has raised the timeout to P2* + margin. An explicit post-await
/// `set_timeout(saved)` only runs on normal completion; Drop also covers
/// cancellation.
struct TimeoutRestore<'chan, 'iface> {
    channel: &'chan mut IsoTpChannel<'iface>,
    saved: Duration,
}

impl<'chan, 'iface> TimeoutRestore<'chan, 'iface> {
    fn new(channel: &'chan mut IsoTpChannel<'iface>) -> Self {
        let saved = channel.timeout();
        Self { channel, saved }
    }
}

impl<'iface> Deref for TimeoutRestore<'_, 'iface> {
    type Target = IsoTpChannel<'iface>;
    fn deref(&self) -> &Self::Target {
        self.channel
    }
}

impl<'iface> DerefMut for TimeoutRestore<'_, 'iface> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.channel
    }
}

impl Drop for TimeoutRestore<'_, '_> {
    fn drop(&mut self) {
        self.channel.set_timeout(self.saved);
    }
}

pub struct UdsClient<'a> {
    channel: IsoTpChannel<'a>,
    p2_star: Duration,
}

impl<'a> UdsClient<'a> {
    pub fn new(interface: &'a mut dyn VehicleInterface, tx_id: u32, rx_id: u32) -> Self {
        Self {
            channel: IsoTpChannel::new(interface, tx_id, rx_id),
            p2_star: DEFAULT_P2_STAR,
        }
    }

    /// Send a raw UDS request and handle NRCs (including NRC 0x78 ResponsePending).
    /// Restores the channel timeout on every exit path -- including cancellation
    /// of the returned future -- via the `TimeoutRestore` guard's `Drop` impl,
    /// even when NRC 0x78 changed it mid-flight.
    pub async fn send_request(&mut self, service: u8, payload: &[u8]) -> Result<Vec<u8>> {
        let mut guard = TimeoutRestore::new(&mut self.channel);
        Self::send_request_inner(&mut guard, self.p2_star, service, payload).await
    }

    async fn send_request_inner(
        channel: &mut IsoTpChannel<'_>,
        p2_star: Duration,
        service: u8,
        payload: &[u8],
    ) -> Result<Vec<u8>> {
        let mut req = vec![service];
        req.extend_from_slice(payload);

        channel.send_payload(&req).await?;

        loop {
            let resp = channel.recv_payload().await?;
            let Some(&sid) = resp.first() else {
                return Err(SterngateError::IsoTpError(
                    "Empty UDS response received".into(),
                ));
            };

            // Check for Negative Response (0x7F)
            if sid == 0x7F {
                let (Some(&rejected_service), Some(&nrc)) = (resp.get(1), resp.get(2)) else {
                    return Err(SterngateError::IsoTpError(
                        "Malformed Negative Response".into(),
                    ));
                };

                // NRC 0x78: RequestCorrectlyReceived-ResponsePending -> ECU asks for more time
                if nrc == 0x78 {
                    channel.set_timeout(p2_star + P2_STAR_MARGIN);
                    continue;
                }

                let desc = Self::lookup_nrc_description(nrc);
                return Err(SterngateError::UdsNegativeResponse {
                    service: rejected_service,
                    nrc,
                    description: desc,
                });
            }

            // Positive response SID is (service + 0x40)
            if sid != service.wrapping_add(0x40) {
                return Err(SterngateError::IsoTpError(format!(
                    "Unexpected response SID: expected 0x{:02X}, got 0x{:02X}",
                    service.wrapping_add(0x40),
                    sid
                )));
            }

            return Ok(resp);
        }
    }

    /// DiagnosticSessionControl (0x10)
    pub async fn diagnostic_session_control(&mut self, session_type: u8) -> Result<Vec<u8>> {
        let resp = self.send_request(0x10, &[session_type]).await?;
        if let Some(p2_star) = p2_star_from(&resp) {
            self.p2_star = p2_star;
        }
        Ok(resp)
    }

    /// SecurityAccess (0x27) with pluggable SeedKeySolver
    pub async fn security_access(&mut self, level: u8, solver: &dyn SeedKeySolver) -> Result<()> {
        let seed_resp = self.send_request(0x27, &[level]).await?;
        if seed_resp.len() < 2 {
            return Err(SterngateError::SecurityAccessDenied(
                "Invalid seed response length".into(),
            ));
        }

        let seed = &seed_resp[2..];
        let key = solver.compute_key(level, seed)?;

        let mut send_key_payload = vec![level + 1];
        send_key_payload.extend_from_slice(&key);

        let key_resp = self.send_request(0x27, &send_key_payload).await?;
        if key_resp.len() < 2 || key_resp[1] != level + 1 {
            return Err(SterngateError::SecurityAccessDenied(
                "Key verification failed".into(),
            ));
        }

        Ok(())
    }

    /// ReadDataByIdentifier (0x22)
    pub async fn read_data_by_identifier(&mut self, did: u16) -> Result<Vec<u8>> {
        let b = did.to_be_bytes();
        self.send_request(0x22, &[b[0], b[1]]).await
    }

    /// WriteDataByIdentifier (0x2E)
    pub async fn write_data_by_identifier(&mut self, did: u16, data: &[u8]) -> Result<Vec<u8>> {
        let b = did.to_be_bytes();
        let mut payload = vec![b[0], b[1]];
        payload.extend_from_slice(data);
        self.send_request(0x2E, &payload).await
    }

    /// ReadDtcInformation (0x19 0x02: reportDTCByStatusMask)
    pub async fn read_dtc_information(
        &mut self,
        status_mask: u8,
        module_name: &str,
    ) -> Result<Vec<Dtc>> {
        let resp = self.send_request(0x19, &[0x02, status_mask]).await?;
        if resp.len() < 3 {
            return Ok(Vec::new());
        }

        let mut dtcs = Vec::new();
        let mut idx = 3; // Skip 0x59, subfunction 0x02, statusAvailabilityMask
        while idx + 3 < resp.len() {
            let b0 = resp[idx];
            let b1 = resp[idx + 1];
            let status = resp[idx + 3];
            dtcs.push(Dtc::parse_iso15031(b0, b1, status, module_name));
            idx += 4;
        }

        Ok(dtcs)
    }

    /// ClearDiagnosticInformation (0x14)
    pub async fn clear_diagnostic_information(&mut self, group_of_dtc: u32) -> Result<()> {
        let b = group_of_dtc.to_be_bytes();
        self.send_request(0x14, &[b[1], b[2], b[3]]).await?;
        Ok(())
    }

    /// TesterPresent (0x3E)
    pub async fn tester_present(&mut self, suppress_pos_rsp: bool) -> Result<()> {
        let sub = if suppress_pos_rsp { 0x80 } else { 0x00 };
        self.send_request(0x3E, &[sub]).await?;
        Ok(())
    }

    /// ECUReset (0x11)
    pub async fn ecu_reset(&mut self, reset_type: u8) -> Result<()> {
        self.send_request(0x11, &[reset_type]).await?;
        Ok(())
    }

    /// RoutineControl (0x31)
    /// sub_function: 0x01 (startRoutine), 0x02 (stopRoutine), 0x03 (requestRoutineResults)
    pub async fn routine_control(
        &mut self,
        sub_function: u8,
        routine_id: u16,
        option_record: &[u8],
    ) -> Result<Vec<u8>> {
        let b = routine_id.to_be_bytes();
        let mut payload = vec![sub_function, b[0], b[1]];
        payload.extend_from_slice(option_record);
        self.send_request(0x31, &payload).await
    }

    /// ReadMemoryByAddress (0x23)
    pub async fn read_memory_by_address(&mut self, address: u32, length: u16) -> Result<Vec<u8>> {
        let addr_bytes = address.to_be_bytes();
        let len_bytes = length.to_be_bytes();
        let payload = vec![
            0x24,
            addr_bytes[0],
            addr_bytes[1],
            addr_bytes[2],
            addr_bytes[3],
            len_bytes[0],
            len_bytes[1],
        ];
        let resp = self.send_request(0x23, &payload).await?;
        if resp.len() > 1 {
            Ok(resp[1..].to_vec())
        } else {
            Ok(Vec::new())
        }
    }

    /// WriteMemoryByAddress (0x3D)
    pub async fn write_memory_by_address(&mut self, address: u32, data: &[u8]) -> Result<Vec<u8>> {
        let addr_bytes = address.to_be_bytes();
        // The ALFID below declares a 2-byte length field: a longer payload would
        // be truncated modulo 65536 and the ECU would write the wrong byte count.
        let len_bytes = u16::try_from(data.len())
            .map_err(|_| {
                SterngateError::ProtocolError(
                    "WriteMemoryByAddress payload exceeds 65535 bytes".into(),
                )
            })?
            .to_be_bytes();
        let mut payload = vec![
            0x24,
            addr_bytes[0],
            addr_bytes[1],
            addr_bytes[2],
            addr_bytes[3],
            len_bytes[0],
            len_bytes[1],
        ];
        payload.extend_from_slice(data);
        self.send_request(0x3D, &payload).await
    }

    fn lookup_nrc_description(nrc: u8) -> String {
        match nrc {
            0x10 => "General Reject".into(),
            0x11 => "Service Not Supported".into(),
            0x12 => "SubFunction Not Supported".into(),
            0x13 => "Incorrect Message Length Or Invalid Format".into(),
            0x22 => "Conditions Not Correct".into(),
            0x24 => "Request Sequence Error".into(),
            0x31 => "Request Out Of Range".into(),
            0x33 => "Security Access Denied".into(),
            0x35 => "Invalid Key".into(),
            0x36 => "Exceeded Number Of Attempts".into(),
            0x37 => "Required Time Delay Not Expired".into(),
            0x70 => "Upload Download Not Accepted".into(),
            0x71 => "Transfer Data Suspended".into(),
            0x72 => "General Programming Failure".into(),
            0x73 => "Wrong Block Sequence Counter".into(),
            0x78 => "Request Correctly Received - Response Pending".into(),
            0x7E => "SubFunction Not Supported In Active Session".into(),
            0x7F => "Service Not Supported In Active Session".into(),
            _ => format!("Unknown NRC 0x{:02X}", nrc),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScriptedInterface;
    use std::time::Duration;

    const SESSION_OK: &[u8] = &[0x06, 0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]; // P2 50 ms, P2* 5000 ms

    #[tokio::test(start_paused = true)]
    async fn pending_response_waits_p2_star() {
        let mut iface = ScriptedInterface::new()
            .rule(0x10, &[SESSION_OK])
            .rule_delayed(
                0x31,
                Duration::from_millis(4000),
                &[&[0x05, 0x71, 0x01, 0xFF, 0x00, 0x00]],
            );
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        uds.diagnostic_session_control(0x03).await.unwrap();
        // The double answers 0x78 at once; the real reply arrives 4 s later, inside P2* (5 s).
        let resp = uds.routine_control(0x01, 0xFF00, &[]).await.unwrap();
        assert_eq!(resp, vec![0x71, 0x01, 0xFF, 0x00, 0x00]);
    }

    #[tokio::test(start_paused = true)]
    async fn pending_response_times_out_after_p2_star() {
        let mut iface = ScriptedInterface::new()
            .rule(0x10, &[SESSION_OK])
            .rule(0x31, &[&[0x03, 0x7F, 0x31, 0x78]]);
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        uds.diagnostic_session_control(0x03).await.unwrap();
        let started = tokio::time::Instant::now();
        let err = uds.routine_control(0x01, 0xFF00, &[]).await.unwrap_err();
        assert!(matches!(err, SterngateError::IsoTpTimeout));
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(5400),
            "waited only {waited:?}"
        );
        assert!(waited < Duration::from_millis(7000), "waited {waited:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_restored_after_pending() {
        let mut iface = ScriptedInterface::new()
            .rule(0x10, &[SESSION_OK])
            .rule_once(
                0x31,
                &[
                    &[0x03, 0x7F, 0x31, 0x78],
                    &[0x05, 0x71, 0x01, 0xFF, 0x00, 0x00],
                ],
            );
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        uds.diagnostic_session_control(0x03).await.unwrap();
        uds.routine_control(0x01, 0xFF00, &[]).await.unwrap();
        // A later request that never gets an answer times out at the normal 1500 ms.
        let started = tokio::time::Instant::now();
        assert!(matches!(
            uds.read_data_by_identifier(0x0100).await,
            Err(SterngateError::IsoTpTimeout)
        ));
        assert!(started.elapsed() < Duration::from_millis(2000));
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_request_restores_timeout() {
        let mut iface = ScriptedInterface::new()
            .rule(0x10, &[SESSION_OK])
            .rule(0x31, &[&[0x03, 0x7F, 0x31, 0x78]]);
        let mut uds = UdsClient::new(&mut iface, 0x7E0, 0x7E8);
        uds.diagnostic_session_control(0x03).await.unwrap();
        // The ECU only ever answers ResponsePending, so this request would wait out
        // the full P2* (5.5 s); wrap it in a shorter outer timeout and drop it early.
        let outer = tokio::time::timeout(
            Duration::from_millis(2000),
            uds.routine_control(0x01, 0xFF00, &[]),
        )
        .await;
        assert!(outer.is_err(), "expected the outer timeout to fire first");
        // If the P2*-elevated timeout leaked past cancellation, this unrelated
        // request would also wait ~5.5 s instead of the normal 1500 ms.
        let started = tokio::time::Instant::now();
        assert!(matches!(
            uds.read_data_by_identifier(0x0100).await,
            Err(SterngateError::IsoTpTimeout)
        ));
        assert!(started.elapsed() < Duration::from_millis(2000));
    }

    #[test]
    fn p2_star_widening_does_not_overflow() {
        assert_eq!(
            p2_star_from(&[0x50, 0x03, 0x00, 0x32, 0xFF, 0xFF]),
            Some(Duration::from_millis(655_350))
        );
        assert_eq!(p2_star_from(&[0x50, 0x03]), None);
    }
}
