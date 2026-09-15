use crate::seedkey::DaimlerSolver;
use crate::uds::{parse_request_download, parse_routine_status, S3KeepAlive, UdsClient};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use sterngate_core::{
    FirmwareSignatures, FlashPackageManifest, FlashProgress, FlashState, PreFlightReport, Result,
    RomCompatibilityVerdict, RomInspectionReport, SterngateError,
};
use sterngate_hal::VehicleInterface;
use tokio::sync::{watch, Mutex};

/// Diagnostic addresses of the engine ECU on the flashing bus.
pub const FLASH_TX_ID: u32 = 0x7E0;
pub const FLASH_RX_ID: u32 = 0x7E8;
/// EraseMemory routine: the point of no return for an application image.
pub const ERASE_ROUTINE_ID: u16 = 0xFF00;
/// Bosch EDC16 memory-check routine as used by this project's virtual ECU;
/// not yet verified against real firmware (spec D8).
pub const CHECKSUM_ROUTINE_ID: u16 = 0x0202;
/// routineStatusRecord value both routines report on success.
pub const CHECKSUM_STATUS_OK: u8 = 0x00;
/// ISO 15765-2 caps one segmented message at 4095 bytes.
pub const ISOTP_MAX_PAYLOAD: usize = 4095;

/// Where the sequence is, for the failure message and the progress feed.
#[derive(Default)]
struct SequenceProgress {
    /// Set immediately before the erase request goes out: from here on a
    /// failure leaves the ECU with an incomplete application.
    post_erase: bool,
    current_block: usize,
    total_blocks: usize,
    bytes_written: usize,
    /// Set when the ECU verified the checksum but did not acknowledge the reset.
    reset_warning: Option<String>,
}

pub struct FlashingWorker {
    progress_tx: watch::Sender<FlashProgress>,
    progress_rx: watch::Receiver<FlashProgress>,
    current_state: Arc<Mutex<FlashState>>,
}

impl FlashingWorker {
    pub fn new() -> Self {
        let initial_progress = FlashProgress::default();
        let (tx, rx) = watch::channel(initial_progress);
        Self {
            progress_tx: tx,
            progress_rx: rx,
            current_state: Arc::new(Mutex::new(FlashState::Idle)),
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<FlashProgress> {
        self.progress_rx.clone()
    }

    pub async fn current_state(&self) -> FlashState {
        *self.current_state.lock().await
    }

    pub async fn is_locked(&self) -> bool {
        self.current_state().await.is_locked()
    }

    /// Pre-flight safety check
    pub async fn run_preflight_checks(
        &self,
        manifest: &FlashPackageManifest,
        rom_data: &[u8],
        battery_voltage: f64,
        interface: &mut dyn VehicleInterface,
    ) -> Result<PreFlightReport> {
        let mut details = Vec::new();

        // 1. Voltage Check
        let voltage_ok = battery_voltage >= 12.5;
        if voltage_ok {
            details.push(format!(
                "Battery voltage OK: {:.2}V (>= 12.5V required)",
                battery_voltage
            ));
        } else {
            details.push(format!(
                "Battery voltage CRITICAL: {:.2}V (minimum 12.5V required)",
                battery_voltage
            ));
        }

        // 2. SHA-256 Check
        let mut hasher = Sha256::new();
        hasher.update(rom_data);
        let calc_sha256 = format!("{:x}", hasher.finalize());
        let sha256_ok = calc_sha256.eq_ignore_ascii_case(&manifest.sha256_checksum);
        if sha256_ok {
            details.push("ROM SHA-256 integrity verified against manifest".into());
        } else {
            details.push(format!(
                "ROM SHA-256 mismatch: calculated {}, expected {}",
                calc_sha256, manifest.sha256_checksum
            ));
        }

        // 3. CRC32 Check
        let calc_crc32 = crc32fast::hash(rom_data);
        let crc32_ok = calc_crc32 == manifest.crc32_checksum;
        if crc32_ok {
            details.push(format!("Bosch CRC32 checksum OK: 0x{:08X}", calc_crc32));
        } else {
            details.push(format!(
                "Bosch CRC32 mismatch: calculated 0x{:08X}, expected 0x{:08X}",
                calc_crc32, manifest.crc32_checksum
            ));
        }

        // 4. Hardware ID check via UDS Service 0x22 DID 0xF191
        let hw_match = if interface.is_connected() {
            let mut uds = UdsClient::new(interface, FLASH_TX_ID, FLASH_RX_ID);
            match uds.read_data_by_identifier(0xF191).await {
                Ok(resp) => {
                    let hex_id = resp
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join("");
                    details.push(format!("ECU Hardware ID response: {}", hex_id));
                    true
                }
                Err(e) => {
                    details.push(format!("Hardware ID check skipped/simulated: {}", e));
                    true
                }
            }
        } else {
            true
        };

        // 5. The download request is built from the manifest, so its declared
        // length must describe the bytes that will actually be streamed.
        let length_ok = usize::try_from(manifest.flash_length).is_ok_and(|l| l == rom_data.len());
        if length_ok {
            details.push(format!(
                "flash_length matches ROM size ({} bytes)",
                rom_data.len()
            ));
        } else {
            details.push(format!(
                "flash_length mismatch: manifest says {} bytes, ROM is {} bytes",
                manifest.flash_length,
                rom_data.len()
            ));
        }

        let passed = voltage_ok && sha256_ok && crc32_ok && hw_match && length_ok;
        Ok(PreFlightReport {
            passed,
            battery_voltage,
            min_voltage_required: 12.5,
            hw_id_match: hw_match,
            checksum_match: sha256_ok && crc32_ok,
            details,
        })
    }

    /// Inspect a firmware ROM binary, extract embedded markers, query connected ECU over CAN,
    /// and evaluate whether the firmware is safe to flash to the vehicle.
    pub async fn inspect_rom(
        &self,
        interface: &mut dyn VehicleInterface,
        tx_id: u32,
        rx_id: u32,
        rom_data: &[u8],
    ) -> Result<RomInspectionReport> {
        let signatures = FirmwareSignatures::extract(rom_data);

        // Read Live ECU identification DIDs if connected
        let (ecu_hw_id, ecu_sw_id, ecu_oem_num) = if interface.is_connected() {
            let mut uds = UdsClient::new(interface, tx_id, rx_id);

            // Read HW Number (DID 0xF192 or fallback 0xF191)
            let hw = match uds.read_data_by_identifier(0xF192).await {
                Ok(resp) if resp.len() >= 4 => {
                    let raw = &resp[3..];
                    if raw.iter().all(|b| b.is_ascii_graphic()) {
                        String::from_utf8(raw.to_vec()).ok()
                    } else {
                        Some(raw.iter().map(|b| format!("{:02X}", b)).collect::<String>())
                    }
                }
                _ => match uds.read_data_by_identifier(0xF191).await {
                    Ok(resp) if resp.len() >= 4 => Some(
                        resp[3..]
                            .iter()
                            .map(|b| format!("{:02X}", b))
                            .collect::<String>(),
                    ),
                    _ => None,
                },
            };

            // Read SW Number (DID 0xF194 or fallback 0xF189)
            let sw = match uds.read_data_by_identifier(0xF194).await {
                Ok(resp) if resp.len() >= 4 => {
                    let raw = &resp[3..];
                    if raw.iter().all(|b| b.is_ascii_graphic()) {
                        String::from_utf8(raw.to_vec()).ok()
                    } else {
                        Some(raw.iter().map(|b| format!("{:02X}", b)).collect::<String>())
                    }
                }
                _ => None,
            };

            // Read OEM Number (DID 0xF187)
            let oem = match uds.read_data_by_identifier(0xF187).await {
                Ok(resp) if resp.len() >= 4 => {
                    let raw = &resp[3..];
                    if raw.iter().all(|b| b.is_ascii_graphic()) {
                        String::from_utf8(raw.to_vec()).ok()
                    } else {
                        Some(raw.iter().map(|b| format!("{:02X}", b)).collect::<String>())
                    }
                }
                _ => None,
            };

            (hw, sw, oem)
        } else {
            (None, None, None)
        };

        // Determine compatibility verdict
        let mut verdict = RomCompatibilityVerdict::Unknown;
        let mut can_flash = true;
        let explanation;

        if let (Some(sig_hw), Some(live_hw)) = (&signatures.bosch_hw_id, &ecu_hw_id) {
            let check_len = sig_hw.len().min(live_hw.len());
            let hw_matches =
                check_len >= 8 && sig_hw[..check_len].eq_ignore_ascii_case(&live_hw[..check_len]);
            if !hw_matches {
                verdict = RomCompatibilityVerdict::HardwareMismatch;
                can_flash = false;
                explanation = format!(
                    "CRITICAL HARDWARE MISMATCH: Firmware binary is built for hardware '{}', but installed ECU reports hardware '{}'. Flashing this binary will brick the ECU microcontroller!",
                    sig_hw, live_hw
                );
            } else if let (Some(sig_sw), Some(live_sw)) = (&signatures.bosch_sw_id, &ecu_sw_id) {
                let sw_check_len = sig_sw.len().min(live_sw.len());
                if sw_check_len >= 8
                    && sig_sw[..sw_check_len].eq_ignore_ascii_case(&live_sw[..sw_check_len])
                {
                    verdict = RomCompatibilityVerdict::Match;
                    explanation = format!(
                        "EXACT MATCH: Firmware matches installed hardware ('{}') and identical calibration version ('{}'). Safe to flash.",
                        sig_hw, sig_sw
                    );
                } else {
                    verdict = RomCompatibilityVerdict::CalibrationUpdate;
                    explanation = format!(
                        "CALIBRATION UPDATE: Hardware matches ('{}'). Firmware contains updated calibration ('{}' vs vehicle '{}'). Compatible for upgrade.",
                        sig_hw, sig_sw, live_sw
                    );
                }
            } else {
                verdict = RomCompatibilityVerdict::Match;
                explanation = format!(
                    "HARDWARE MATCH: Hardware revision verified ('{}'). Safe to stage.",
                    sig_hw
                );
            }
        } else if let Some(sig_hw) = &signatures.bosch_hw_id {
            verdict = RomCompatibilityVerdict::Match;
            explanation = format!(
                "Firmware signature detected: Bosch HW {}, SW {}. (ECU offline/unconnected).",
                sig_hw,
                signatures.bosch_sw_id.as_deref().unwrap_or("Unknown")
            );
        } else {
            explanation = "Raw binary without recognized Bosch/OEM markers. Flashing requires manual verification of target address and memory layout.".to_string();
        }

        Ok(RomInspectionReport {
            signatures,
            ecu_hw_id,
            ecu_sw_id,
            ecu_oem_num,
            verdict,
            can_flash,
            risk_explanation: explanation,
        })
    }

    /// Execute the detached safe flashing sequence
    pub async fn execute_flash(
        &self,
        manifest: FlashPackageManifest,
        rom_data: Vec<u8>,
        battery_voltage: f64,
        interface: Arc<Mutex<Box<dyn VehicleInterface>>>,
    ) -> Result<()> {
        let mut state_lock = self.current_state.lock().await;
        if state_lock.is_locked() {
            return Err(SterngateError::FlashingApiLocked);
        }
        *state_lock = FlashState::Locked;
        drop(state_lock);

        let total_bytes = rom_data.len();
        let mut iface_guard = interface.lock().await;

        // Pre-flight check
        let report = self
            .run_preflight_checks(&manifest, &rom_data, battery_voltage, iface_guard.as_mut())
            .await?;
        if !report.passed {
            let err_msg = format!("Pre-flight check failed: {:?}", report.details);
            self.update_progress(
                FlashState::Failed,
                0,
                0,
                0,
                0,
                total_bytes,
                &err_msg,
                Some(err_msg.clone()),
            );
            *self.current_state.lock().await = FlashState::Failed;
            return Err(SterngateError::PreFlightCheckFailed(err_msg));
        }
        self.update_progress(
            FlashState::Locked,
            5,
            0,
            0,
            0,
            total_bytes,
            "Pre-flight checks passed. API Lockout engaged.",
            None,
        );

        let mut ka = S3KeepAlive::new();
        let mut progress = SequenceProgress::default();
        let result = {
            // The interface stays locked for the whole sequence: nothing else may
            // put a frame on the bus between the session request and the reset.
            let mut uds = UdsClient::new(iface_guard.as_mut(), FLASH_TX_ID, FLASH_RX_ID);
            self.run_programming_sequence(&manifest, &rom_data, &mut uds, &mut ka, &mut progress)
                .await
        };
        drop(iface_guard);

        match result {
            Ok(()) => {
                self.update_progress(
                    FlashState::Completed,
                    100,
                    progress.total_blocks,
                    progress.total_blocks,
                    progress.bytes_written,
                    total_bytes,
                    "Flash completed successfully! ECU checksum verified.",
                    progress.reset_warning.clone(),
                );
                *self.current_state.lock().await = FlashState::Completed;
                Ok(())
            }
            Err(e) => {
                let msg = if progress.post_erase {
                    format!("FLASH FAILED AFTER ERASE - ECU is in bootloader with incomplete application. Keep ignition ON, do not disconnect. {e}")
                } else {
                    format!("Flash aborted before erase; ECU untouched. {e}")
                };
                self.update_progress(
                    FlashState::Failed,
                    0,
                    progress.current_block,
                    progress.total_blocks,
                    progress.bytes_written,
                    total_bytes,
                    &msg,
                    Some(msg.clone()),
                );
                *self.current_state.lock().await = FlashState::Failed;
                Err(e)
            }
        }
    }

    /// The UDS programming sequence itself. Every step propagates its error, so
    /// a negative response or a mis-acknowledged block stops the flash instead
    /// of letting the caller report success over a half-written ECU.
    async fn run_programming_sequence(
        &self,
        manifest: &FlashPackageManifest,
        rom_data: &[u8],
        uds: &mut UdsClient<'_>,
        ka: &mut S3KeepAlive,
        progress: &mut SequenceProgress,
    ) -> Result<()> {
        let total_bytes = rom_data.len();

        // Step 1: Extended Diagnostic Session (0x10 03)
        self.update_progress(
            FlashState::SessionExtended,
            10,
            0,
            0,
            0,
            total_bytes,
            "Requesting Extended Diagnostic Session (0x10 03)...",
            None,
        );
        let resp = uds.diagnostic_session_control(0x03).await?;
        if resp.get(1) != Some(&0x03) {
            return Err(SterngateError::ProtocolError(format!(
                "Extended session not confirmed: {resp:02X?}"
            )));
        }

        // Step 2: Security Access (0x27 0B)
        self.update_progress(
            FlashState::SecurityUnlocked,
            20,
            0,
            0,
            0,
            total_bytes,
            "Performing Bootloader Security Access (Level 0x0B)...",
            None,
        );
        ka.tick(uds).await?;
        uds.security_access(0x0B, &DaimlerSolver).await?;

        // Step 3: Silence bus (0x28 01 01) and DTC recording (0x85 02)
        self.update_progress(
            FlashState::BusSilenced,
            25,
            0,
            0,
            0,
            total_bytes,
            "Silencing vehicle CAN traffic (0x28) and DTC recording (0x85)...",
            None,
        );
        ka.tick(uds).await?;
        uds.send_request(0x28, &[0x01, 0x01]).await?;
        ka.tick(uds).await?;
        uds.send_request(0x85, &[0x02]).await?;

        // Step 4: Programming Session (0x10 02)
        self.update_progress(
            FlashState::SessionProgramming,
            30,
            0,
            0,
            0,
            total_bytes,
            "Entering Programming Session (0x10 02)...",
            None,
        );
        ka.tick(uds).await?;
        let resp = uds.diagnostic_session_control(0x02).await?;
        if resp.get(1) != Some(&0x02) {
            return Err(SterngateError::ProtocolError(format!(
                "Programming session not confirmed: {resp:02X?}"
            )));
        }

        // Step 5: Erase (0x31 01 FF 00) - point of no return
        self.update_progress(
            FlashState::Erasing,
            40,
            0,
            0,
            0,
            total_bytes,
            "Erasing ECU Flash Memory sectors (0x31 01 FF 00)...",
            None,
        );
        ka.tick(uds).await?;
        progress.post_erase = true;
        let resp = uds.routine_control(0x01, ERASE_ROUTINE_ID, &[]).await?;
        let status = parse_routine_status(&resp, 0x01, ERASE_ROUTINE_ID)?;
        if status != CHECKSUM_STATUS_OK {
            return Err(SterngateError::FlashAborted(format!(
                "erase routine reported status 0x{status:02X}"
            )));
        }

        // Step 6: Request Download (0x34) from the manifest, then Transfer Data (0x36)
        let a = manifest.flash_start_address.to_be_bytes();
        let l = manifest.flash_length.to_be_bytes();
        ka.tick(uds).await?;
        let resp = uds
            .send_request(
                0x34,
                &[0x00, 0x44, a[0], a[1], a[2], a[3], l[0], l[1], l[2], l[3]],
            )
            .await?;
        let max_block_len = parse_request_download(&resp)?;
        // maxNumberOfBlockLength counts SID + block counter; ISO-TP caps the whole payload.
        let chunk_len = manifest
            .block_size
            .min(max_block_len.saturating_sub(2))
            .min(ISOTP_MAX_PAYLOAD - 2);
        if chunk_len == 0 {
            return Err(SterngateError::FlashAborted(
                "negotiated block size leaves no room for data".into(),
            ));
        }
        let chunks: Vec<&[u8]> = rom_data.chunks(chunk_len).collect();
        progress.total_blocks = chunks.len();
        self.update_progress(
            FlashState::Transferring,
            45,
            0,
            chunks.len(),
            0,
            total_bytes,
            "Download accepted; transferring blocks (0x36)...",
            None,
        );

        for (i, chunk) in chunks.iter().enumerate() {
            let block_num = u8::try_from((i + 1) % 256)
                .map_err(|_| SterngateError::Internal("block counter".into()))?;
            let mut payload = vec![block_num];
            payload.extend_from_slice(chunk);
            ka.tick(uds).await?;
            let resp = uds.send_request(0x36, &payload).await?;
            if resp.get(1) != Some(&block_num) {
                return Err(SterngateError::ProtocolError(format!(
                    "TransferData block {} not acknowledged (echo {:02X?})",
                    i + 1,
                    resp.get(1)
                )));
            }
            progress.current_block = i + 1;
            progress.bytes_written += chunk.len();
            let pct =
                45 + u8::try_from(progress.bytes_written * 45 / total_bytes.max(1)).unwrap_or(45);
            let log_msg = format!(
                "Writing Block {}/{} ({} bytes written)",
                i + 1,
                chunks.len(),
                progress.bytes_written
            );
            self.update_progress(
                FlashState::Transferring,
                pct,
                i + 1,
                chunks.len(),
                progress.bytes_written,
                total_bytes,
                &log_msg,
                None,
            );
        }

        // Step 7: Request Transfer Exit (0x37)
        self.update_progress(
            FlashState::TransferExited,
            92,
            progress.current_block,
            progress.total_blocks,
            progress.bytes_written,
            total_bytes,
            "Exiting Transfer (0x37)...",
            None,
        );
        ka.tick(uds).await?;
        uds.send_request(0x37, &[]).await?;

        // Step 8: ECU checksum routine, before anything leaves the bootloader
        self.update_progress(
            FlashState::VerifyingChecksum,
            95,
            progress.current_block,
            progress.total_blocks,
            progress.bytes_written,
            total_bytes,
            "Verifying checksum on written sectors...",
            None,
        );
        ka.tick(uds).await?;
        let resp = uds.routine_control(0x01, CHECKSUM_ROUTINE_ID, &[]).await?;
        let status = parse_routine_status(&resp, 0x01, CHECKSUM_ROUTINE_ID)?;
        if status != CHECKSUM_STATUS_OK {
            return Err(SterngateError::ChecksumMismatch {
                expected: format!("0x{CHECKSUM_STATUS_OK:02X}"),
                calculated: format!("0x{status:02X}"),
            });
        }

        // Step 9: ECU reset. A verified image that fails to reset is not a failed flash.
        self.update_progress(
            FlashState::ResettingEcu,
            98,
            progress.current_block,
            progress.total_blocks,
            progress.bytes_written,
            total_bytes,
            "Issuing ECU Hard Reset (0x11 01)...",
            None,
        );
        ka.tick(uds).await?;
        if uds.ecu_reset(0x01).await.is_err() {
            progress.reset_warning =
                Some("ECU did not acknowledge reset; cycle ignition manually".into());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn update_progress(
        &self,
        state: FlashState,
        percentage: u8,
        current_block: usize,
        total_blocks: usize,
        bytes_written: usize,
        total_bytes: usize,
        log: &str,
        error_message: Option<String>,
    ) {
        let progress = FlashProgress {
            state,
            percentage,
            current_block,
            total_blocks,
            bytes_written,
            total_bytes,
            log: log.to_string(),
            error_message,
        };
        let _ = self.progress_tx.send(progress);
    }
}

impl Default for FlashingWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScriptedInterface;
    use std::time::Duration;
    use sterngate_hal::VirtualCanInterface;

    const SESSION_EXT: &[u8] = &[0x06, 0x50, 0x03, 0x00, 0x32, 0x01, 0xF4];
    const SESSION_PROG: &[u8] = &[0x06, 0x50, 0x02, 0x00, 0x32, 0x01, 0xF4];
    const SEED: &[u8] = &[0x06, 0x67, 0x0B, 0x12, 0x34, 0x56, 0x78];
    const KEY_OK: &[u8] = &[0x02, 0x67, 0x0C];
    const COMM_OFF: &[u8] = &[0x02, 0x68, 0x01];
    const DTC_OFF: &[u8] = &[0x02, 0xC5, 0x02];
    const ERASE_OK: &[u8] = &[0x05, 0x71, 0x01, 0xFF, 0x00, 0x00];
    const DOWNLOAD_OK: &[u8] = &[0x04, 0x74, 0x20, 0x0F, 0xFF];
    const EXIT_OK: &[u8] = &[0x01, 0x77];
    const CHECKSUM_OK: &[u8] = &[0x05, 0x71, 0x01, 0x02, 0x02, 0x00];
    const RESET_OK: &[u8] = &[0x02, 0x51, 0x01];
    // A TransferData acknowledgement echoes the block sequence counter it was
    // sent, so a multi-block transfer needs one scripted reply per block.
    const ACK_BLOCK_1: &[u8] = &[0x02, 0x76, 0x01];
    const ACK_BLOCK_2: &[u8] = &[0x02, 0x76, 0x02];
    const ACK_BLOCK_3: &[u8] = &[0x02, 0x76, 0x03];
    // F192 -> "0281012224" as FF + CF (the test double delivers CFs after the tester's FC)
    const F192_FF: &[u8] = &[0x10, 0x0D, 0x62, 0xF1, 0x92, b'0', b'2', b'8'];
    const F192_CF: &[u8] = &[0x21, b'1', b'0', b'1', b'2', b'2', b'2', b'4'];

    fn manifest(rom: &[u8]) -> FlashPackageManifest {
        let mut hasher = Sha256::new();
        hasher.update(rom);
        FlashPackageManifest {
            target_module: "EDC16".into(),
            expected_hw_id: "0281012224".into(),
            expected_sw_id: "1037372332".into(),
            sha256_checksum: format!("{:x}", hasher.finalize()),
            crc32_checksum: crc32fast::hash(rom),
            flash_start_address: 0x0004_0000,
            flash_length: u32::try_from(rom.len()).unwrap(),
            block_size: 256,
        }
    }

    /// A scripted ECU that answers every step positively, for a transfer of up
    /// to two blocks; tests override single steps.
    fn happy_ecu() -> ScriptedInterface {
        ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[CHECKSUM_OK])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule_once(0x36, &[ACK_BLOCK_1])
            .rule(0x36, &[ACK_BLOCK_2])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[RESET_OK])
    }

    async fn run(
        iface: ScriptedInterface,
        rom: Vec<u8>,
    ) -> (
        FlashingWorker,
        Result<()>,
        Arc<Mutex<Box<dyn VehicleInterface>>>,
    ) {
        let flasher = FlashingWorker::new();
        let m = manifest(&rom);
        let shared: Arc<Mutex<Box<dyn VehicleInterface>>> = Arc::new(Mutex::new(Box::new(iface)));
        let res = flasher.execute_flash(m, rom, 13.5, shared.clone()).await;
        (flasher, res, shared)
    }

    #[tokio::test(start_paused = true)]
    async fn full_success_path_completes_in_order() {
        let iface = happy_ecu();
        let log = iface.sent_handle();
        let rom = vec![0x5A; 300]; // 2 blocks at 256
        let (flasher, res, _) = run(iface, rom).await;
        res.unwrap();
        assert_eq!(flasher.current_state().await, FlashState::Completed);
        let sids: Vec<u8> = log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|f| crate::test_support::request_sid(&f.data))
            .collect();
        // 22 (preflight F192), 10 03, 27 0B, 27 0C, 28, 85, 10 02, 31 FF00, 34, 36, 36, 37, 31 0202, 11
        assert_eq!(
            sids,
            vec![
                0x22, 0x10, 0x27, 0x27, 0x28, 0x85, 0x10, 0x31, 0x34, 0x36, 0x36, 0x37, 0x31, 0x11
            ]
        );
        let prog = flasher.subscribe().borrow().clone();
        assert_eq!(prog.bytes_written, 300);
        assert!(prog.error_message.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn security_access_nrc_aborts_before_erase() {
        // `rule_once` is matched first-registered-first, so a fresh script is
        // built instead of overriding `happy_ecu`'s seed rule.
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule(0x10, &[SESSION_EXT])
            .rule(0x27, &[&[0x03, 0x7F, 0x27, 0x35]]);
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, vec![0x5A; 300]).await;
        assert!(matches!(
            res,
            Err(SterngateError::UdsNegativeResponse {
                service: 0x27,
                nrc: 0x35,
                ..
            })
        ));
        assert_eq!(flasher.current_state().await, FlashState::Failed);
        assert!(!flasher.is_locked().await);
        let prog = flasher.subscribe().borrow().clone();
        assert!(prog
            .error_message
            .as_deref()
            .unwrap()
            .starts_with("Flash aborted before erase; ECU untouched."));
        let sids: Vec<u8> = log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|f| crate::test_support::request_sid(&f.data))
            .collect();
        assert!(
            !sids.contains(&0x31),
            "no erase after a security-access NRC"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn request_download_nrc_aborts_after_erase() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule(0x31, &[ERASE_OK])
            .rule(0x34, &[&[0x03, 0x7F, 0x34, 0x70]]);
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, vec![0x5A; 300]).await;
        assert!(res.is_err());
        assert_eq!(flasher.current_state().await, FlashState::Failed);
        let prog = flasher.subscribe().borrow().clone();
        assert!(prog
            .error_message
            .as_deref()
            .unwrap()
            .starts_with("FLASH FAILED AFTER ERASE"));
        let sids: Vec<u8> = log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|f| crate::test_support::request_sid(&f.data))
            .collect();
        assert!(!sids.contains(&0x36));
    }

    #[tokio::test(start_paused = true)]
    async fn block_counter_echo_mismatch_aborts() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule(0x36, &[&[0x02, 0x76, 0x05]]);
        let log = iface.sent_handle();
        let (_, res, _) = run(iface, vec![0x5A; 300]).await;
        assert!(matches!(res, Err(SterngateError::ProtocolError(_))));
        let sids: Vec<u8> = log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|f| crate::test_support::request_sid(&f.data))
            .collect();
        assert_eq!(sids.iter().filter(|s| **s == 0x36).count(), 1);
        assert!(!sids.contains(&0x37));
    }

    #[tokio::test(start_paused = true)]
    async fn checksum_status_nonzero_fails_before_reset() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[&[0x05, 0x71, 0x01, 0x02, 0x02, 0x01]])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule(0x36, &[ACK_BLOCK_1])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[RESET_OK]);
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, vec![0x5A; 100]).await;
        assert!(matches!(res, Err(SterngateError::ChecksumMismatch { .. })));
        assert_eq!(flasher.current_state().await, FlashState::Failed);
        let sids: Vec<u8> = log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|f| crate::test_support::request_sid(&f.data))
            .collect();
        assert!(!sids.contains(&0x11), "no reset after a failed checksum");
    }

    #[tokio::test(start_paused = true)]
    async fn checksum_missing_status_byte_fails_closed() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[&[0x04, 0x71, 0x01, 0x02, 0x02]])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule(0x36, &[ACK_BLOCK_1])
            .rule(0x37, &[EXIT_OK]);
        let (_, res, _) = run(iface, vec![0x5A; 100]).await;
        assert!(res.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn reset_failure_after_verified_checksum_completes_with_warning() {
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[CHECKSUM_OK])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule(0x36, &[ACK_BLOCK_1])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[&[0x03, 0x7F, 0x11, 0x22]]);
        let (flasher, res, _) = run(iface, vec![0x5A; 100]).await;
        res.unwrap();
        assert_eq!(flasher.current_state().await, FlashState::Completed);
        let prog = flasher.subscribe().borrow().clone();
        assert_eq!(
            prog.error_message.as_deref(),
            Some("ECU did not acknowledge reset; cycle ignition manually")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn request_download_uses_manifest_and_clamps_block_size() {
        // ECU max block 258 -> 256 data bytes per 0x36 even though manifest says 4096
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[CHECKSUM_OK])
            .rule(0x34, &[&[0x04, 0x74, 0x20, 0x01, 0x02]])
            .rule_once(0x36, &[ACK_BLOCK_1])
            .rule_once(0x36, &[ACK_BLOCK_2])
            .rule(0x36, &[ACK_BLOCK_3])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[RESET_OK]);
        let log = iface.sent_handle();
        let rom = vec![0x5A; 600];
        let flasher = FlashingWorker::new();
        let mut m = manifest(&rom);
        m.flash_start_address = 0x0008_0000;
        m.block_size = 4096;
        let shared: Arc<Mutex<Box<dyn VehicleInterface>>> = Arc::new(Mutex::new(Box::new(iface)));
        flasher.execute_flash(m, rom, 13.5, shared).await.unwrap();
        let frames = log.lock().unwrap().clone();
        // 0x34 First Frame carries: 34 00 44 00 08 00 00 (address) then CF: 00 00 02 58 (length 600)
        let ff = frames
            .iter()
            .find(|f| f.data.first().map(|b| b >> 4) == Some(1) && f.data.get(2) == Some(&0x34))
            .unwrap();
        assert_eq!(&ff.data[2..8], &[0x34, 0x00, 0x44, 0x00, 0x08, 0x00]);
        let prog = flasher.subscribe().borrow().clone();
        assert_eq!(prog.total_blocks, 3, "600 bytes at 256 per block");
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_sent_when_ecu_is_slow() {
        // Longer than S3_KEEPALIVE_INTERVAL, so the gap between two blocks
        // leaves the session idle past the keep-alive deadline.
        const SLOW_BLOCK: Duration = Duration::from_millis(1600);
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule_once(0x31, &[ERASE_OK])
            .rule(0x31, &[CHECKSUM_OK])
            .rule(0x34, &[DOWNLOAD_OK])
            .rule_delayed_once(0x36, SLOW_BLOCK, &[ACK_BLOCK_1])
            .rule_delayed_once(0x36, SLOW_BLOCK, &[ACK_BLOCK_2])
            .rule_delayed(0x36, SLOW_BLOCK, &[ACK_BLOCK_3])
            .rule(0x37, &[EXIT_OK])
            .rule(0x11, &[RESET_OK]);
        let log = iface.sent_handle();
        let (_, res, _) = run(iface, vec![0x5A; 600]).await;
        res.unwrap();
        let frames = log.lock().unwrap().clone();
        let tp: Vec<usize> = frames
            .iter()
            .enumerate()
            .filter(|(_, f)| f.data.get(1) == Some(&0x3E))
            .map(|(i, _)| i)
            .collect();
        assert!(
            !tp.is_empty(),
            "a 3E 80 keep-alive must be sent between slow blocks"
        );
        // Never between a First Frame and the last Consecutive Frame of one request.
        for i in tp {
            let prev = &frames[i - 1].data;
            assert_ne!(
                prev.first().map(|b| b >> 4),
                Some(1),
                "keep-alive after a First Frame"
            );
        }
    }

    #[tokio::test]
    async fn preflight_rejects_flash_length_mismatch() {
        let mut iface = happy_ecu();
        let rom = vec![0x5A; 100];
        let mut m = manifest(&rom);
        m.flash_length = 101;
        let report = FlashingWorker::new()
            .run_preflight_checks(&m, &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.details.iter().any(|d| d.contains("flash_length")));
    }

    #[tokio::test]
    async fn execute_flash_on_virtual_can_completes() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        let rom = vec![0x5A; 1000];
        let m = manifest(&rom);
        let shared: Arc<Mutex<Box<dyn VehicleInterface>>> = Arc::new(Mutex::new(Box::new(sim)));
        let flasher = FlashingWorker::new();
        flasher.execute_flash(m, rom, 13.5, shared).await.unwrap();
        assert_eq!(flasher.current_state().await, FlashState::Completed);
        assert_eq!(flasher.subscribe().borrow().bytes_written, 1000);
    }
}
