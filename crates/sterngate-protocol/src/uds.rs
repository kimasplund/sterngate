use crate::isotp::IsoTpChannel;
use crate::seedkey::SeedKeySolver;
use sterngate_core::{Dtc, Result, SterngateError};
use sterngate_hal::VehicleInterface;

pub struct UdsClient<'a> {
    channel: IsoTpChannel<'a>,
}

impl<'a> UdsClient<'a> {
    pub fn new(interface: &'a mut dyn VehicleInterface, tx_id: u32, rx_id: u32) -> Self {
        Self {
            channel: IsoTpChannel::new(interface, tx_id, rx_id),
        }
    }

    /// Send a raw UDS request and handle NRCs (including NRC 0x78 ResponsePending)
    pub async fn send_request(&mut self, service: u8, payload: &[u8]) -> Result<Vec<u8>> {
        let mut req = vec![service];
        req.extend_from_slice(payload);

        self.channel.send_payload(&req).await?;

        loop {
            let resp = self.channel.recv_payload().await?;
            if resp.is_empty() {
                return Err(SterngateError::IsoTpError(
                    "Empty UDS response received".into(),
                ));
            }

            // Check for Negative Response (0x7F)
            if resp[0] == 0x7F {
                if resp.len() < 3 {
                    return Err(SterngateError::IsoTpError(
                        "Malformed Negative Response".into(),
                    ));
                }
                let rejected_service = resp[1];
                let nrc = resp[2];

                // NRC 0x78: RequestCorrectlyReceived-ResponsePending -> ECU asks for more time
                if nrc == 0x78 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
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
            if resp[0] != service + 0x40 {
                return Err(SterngateError::IsoTpError(format!(
                    "Unexpected response SID: expected 0x{:02X}, got 0x{:02X}",
                    service + 0x40,
                    resp[0]
                )));
            }

            return Ok(resp);
        }
    }

    /// DiagnosticSessionControl (0x10)
    pub async fn diagnostic_session_control(&mut self, session_type: u8) -> Result<Vec<u8>> {
        self.send_request(0x10, &[session_type]).await
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
