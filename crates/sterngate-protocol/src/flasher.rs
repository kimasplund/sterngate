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

/// Read the system-supplier ECU hardware number (DID 0xF192, the Bosch
/// `0281…` number).
///
/// The field is fixed-length, so real ECUs pad it: trailing spaces, NULs or
/// erased-flash `0xFF` are stripped before the payload is classified. What is
/// left is returned as text if it is printable, and otherwise rendered as hex
/// digits. Hex is what keeps the later comparison exact and explicit rather
/// than encoding-dependent: a binary-coded `02 81 01 22 24` renders as
/// `0281012224` and so matches a manifest declaring that same number, which is
/// the intended equivalence.
pub async fn read_supplier_hw_id(
    interface: &mut dyn VehicleInterface,
    tx_id: u32,
    rx_id: u32,
) -> Result<String> {
    if !interface.is_connected() {
        return Err(SterngateError::DeviceNotFound(
            "interface not connected".into(),
        ));
    }
    let mut uds = UdsClient::new(interface, tx_id, rx_id);
    let resp = uds.read_data_by_identifier(0xF192).await?;
    if resp.len() <= 3 || resp.get(1..3) != Some(&[0xF1, 0x92]) {
        return Err(SterngateError::IsoTpError(format!(
            "F192 reply malformed: {resp:02X?}"
        )));
    }
    let payload = &resp[3..];
    let Some(last) = payload
        .iter()
        .rposition(|&b| !matches!(b, 0x00 | 0x20 | 0xFF))
    else {
        return Err(SterngateError::IsoTpError(format!(
            "F192 reply carries padding only, no hardware number: {resp:02X?}"
        )));
    };
    let unpadded = payload.get(..=last).unwrap_or(payload);
    // Asymmetric on purpose: an ASCII number is compared unpadded, a binary
    // (e.g. BCD) one is rendered with its padding. A padded BCD reply can
    // therefore never equal a manifest id, which fails the flash closed --
    // correct until a real EDC16 BCD F192 reply is captured on the bench.
    if unpadded.iter().all(u8::is_ascii_graphic) {
        Ok(String::from_utf8_lossy(unpadded).to_string())
    } else {
        // Not text: render every byte as received, padding included, so the
        // number that is compared is the number the ECU reported.
        Ok(payload.iter().map(|b| format!("{b:02X}")).collect())
    }
}

/// Exact hardware-number comparison. Bosch numbers differ in their last digits
/// between hardware variants, so a prefix rule would accept a sibling ECU.
pub fn hw_id_matches(expected: &str, live: &str) -> bool {
    let e = expected.trim().to_ascii_uppercase();
    let l = live.trim().to_ascii_uppercase();
    e.len() >= 8 && l.len() >= 8 && e == l
}

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

        // 4. Hardware identity: the ECU's supplier number must equal the manifest's.
        let hw_match = if manifest.expected_hw_id.trim().is_empty() {
            details.push(
                "Hardware ID check FAILED (fail-closed): manifest declares no expected_hw_id"
                    .into(),
            );
            false
        } else {
            match read_supplier_hw_id(interface, FLASH_TX_ID, FLASH_RX_ID).await {
                Ok(live) => {
                    let ok = hw_id_matches(&manifest.expected_hw_id, &live);
                    details.push(format!(
                        "ECU F192 supplier HW '{live}' vs manifest '{}' -> {}",
                        manifest.expected_hw_id,
                        if ok { "MATCH" } else { "MISMATCH" }
                    ));
                    ok
                }
                Err(e) => {
                    details.push(format!("Hardware ID check FAILED (fail-closed): {e}"));
                    false
                }
            }
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

        // 6. A zero-length image would erase the ECU and then write nothing,
        // leaving a bare bootloader. Every other check passes trivially on an
        // empty ROM (its own hash and CRC32 match a manifest derived from it),
        // so the emptiness has to be rejected on its own.
        let non_empty = !rom_data.is_empty();
        if !non_empty {
            details.push("ROM image is empty (0 bytes); refusing to flash".into());
        }

        // 7. A zero block size clamps the transfer chunk to nothing. The
        // sequence catches that too, but only after the erase has run, and this
        // is fully knowable before the point of no return. Reachable from
        // outside: POST /api/v1/flash/stage passes the body's block_size
        // straight through, and the CLI deserialises the manifest unvalidated.
        let block_size_ok = manifest.block_size > 0;
        if !block_size_ok {
            details.push("manifest block_size is 0; refusing to flash".into());
        }

        // 8. A Caesar flash container is not a raw image; its bytes must never be written verbatim.
        let is_container = sterngate_core::cff::sniff(rom_data);
        if is_container {
            details.push("ROM is a Caesar flash container (.cff), not a raw image; refusing to flash. Extract a verified segment first (Phase 1).".into());
        }

        let passed = voltage_ok
            && sha256_ok
            && crc32_ok
            && hw_match
            && length_ok
            && non_empty
            && block_size_ok
            && !is_container;
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
            // The supplier hardware number (F192) is the identity this report
            // stands on; F191 is the OEM's own number in a different namespace
            // and is never a substitute for it.
            let hw = read_supplier_hw_id(interface, tx_id, rx_id).await.ok();

            let mut uds = UdsClient::new(interface, tx_id, rx_id);

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
        // Fail closed: nothing is flashable until a live F192 read has confirmed
        // the ROM was built for the hardware that is actually installed.
        let mut can_flash = false;
        let explanation;

        if let (Some(sig_hw), Some(live_hw)) = (&signatures.bosch_hw_id, &ecu_hw_id) {
            let hw_matches = hw_id_matches(sig_hw, live_hw);
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
                    can_flash = true;
                    explanation = format!(
                        "EXACT MATCH: Firmware matches installed hardware ('{}') and identical calibration version ('{}'). Safe to flash.",
                        sig_hw, sig_sw
                    );
                } else {
                    verdict = RomCompatibilityVerdict::CalibrationUpdate;
                    can_flash = true;
                    explanation = format!(
                        "CALIBRATION UPDATE: Hardware matches ('{}'). Firmware contains updated calibration ('{}' vs vehicle '{}'). Compatible for upgrade.",
                        sig_hw, sig_sw, live_sw
                    );
                }
            } else {
                verdict = RomCompatibilityVerdict::Match;
                can_flash = true;
                explanation = format!(
                    "HARDWARE MATCH: Hardware revision verified ('{}'). Safe to stage.",
                    sig_hw
                );
            }
        } else if let Some(sig_hw) = &signatures.bosch_hw_id {
            if interface.is_connected() {
                verdict = RomCompatibilityVerdict::Unknown;
                explanation = format!(
                    "ECU connected but did not return a supplier hardware number (F192); cannot confirm firmware '{sig_hw}' matches the installed hardware."
                );
            } else {
                verdict = RomCompatibilityVerdict::Unknown;
                explanation = format!(
                    "Firmware signature detected: Bosch HW {sig_hw}, SW {}. ECU offline: identity not verified, not flashable.",
                    signatures.bosch_sw_id.as_deref().unwrap_or("Unknown")
                );
            }
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
        let mut ka = S3KeepAlive::new();
        let mut progress = SequenceProgress::default();

        // Pre-flight and the sequence share one lock and one failure path: an
        // error raised before the erase must never escape past the state
        // assignment below, or the API lockout would stay engaged forever.
        let result = {
            let mut iface_guard = interface.lock().await;
            let preflight = self
                .run_preflight_checks(&manifest, &rom_data, battery_voltage, iface_guard.as_mut())
                .await;
            match preflight {
                Err(e) => Err(e),
                Ok(report) if !report.passed => Err(SterngateError::PreFlightCheckFailed(format!(
                    "{:?}",
                    report.details
                ))),
                Ok(_) => {
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
                    // The interface stays locked for the whole sequence: nothing
                    // else may put a frame on the bus between the session request
                    // and the reset.
                    let mut uds = UdsClient::new(iface_guard.as_mut(), FLASH_TX_ID, FLASH_RX_ID);
                    self.run_programming_sequence(
                        &manifest,
                        &rom_data,
                        &mut uds,
                        &mut ka,
                        &mut progress,
                    )
                    .await
                }
            }
        };

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
        // Defence in depth: pre-flight already refused a zero block_size, so what
        // is left here is an ECU that answered maxNumberOfBlockLength <= 2.
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
    async fn preflight_rejects_empty_rom() {
        // Every other check passes on an empty ROM: it hashes to a manifest
        // derived from itself, and flash_length 0 matches its length.
        let mut iface = happy_ecu();
        let rom = Vec::new();
        let m = manifest(&rom);
        let report = FlashingWorker::new()
            .run_preflight_checks(&m, &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report
            .details
            .iter()
            .any(|d| d == "ROM image is empty (0 bytes); refusing to flash"));
    }

    #[tokio::test(start_paused = true)]
    async fn empty_rom_never_reaches_the_erase() {
        let iface = happy_ecu();
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, Vec::new()).await;
        assert!(matches!(res, Err(SterngateError::PreFlightCheckFailed(_))));
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
            "an empty ROM must never erase the ECU"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn error_before_erase_releases_the_lockout() {
        // A disconnected interface fails on the first frame of the sequence.
        // Whatever raises it, an error before the erase must leave the worker
        // Failed and unlocked, never stranded in Locked.
        let iface = happy_ecu().disconnected();
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, vec![0x5A; 300]).await;
        assert!(res.is_err());
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
        assert!(!sids.contains(&0x31));
    }

    #[tokio::test]
    async fn preflight_rejects_zero_block_size() {
        let mut iface = happy_ecu();
        let rom = vec![0x5A; 100];
        let mut m = manifest(&rom);
        m.block_size = 0;
        let report = FlashingWorker::new()
            .run_preflight_checks(&m, &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report
            .details
            .iter()
            .any(|d| d == "manifest block_size is 0; refusing to flash"));
    }

    #[tokio::test(start_paused = true)]
    async fn zero_block_size_never_reaches_the_erase() {
        // The in-sequence clamp also catches this, but only once the ECU has
        // already been erased; it has to be refused before the point of no return.
        let iface = happy_ecu();
        let log = iface.sent_handle();
        let rom = vec![0x5A; 300];
        let mut m = manifest(&rom);
        m.block_size = 0;
        let flasher = FlashingWorker::new();
        let shared: Arc<Mutex<Box<dyn VehicleInterface>>> = Arc::new(Mutex::new(Box::new(iface)));
        let res = flasher.execute_flash(m, rom, 13.5, shared).await;
        assert!(matches!(res, Err(SterngateError::PreFlightCheckFailed(_))));
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
            "a zero block size must never erase the ECU"
        );
    }

    /// Minimal synthetic Caesar container: prologue, NUL padding, stub magic. No firmware bytes.
    fn synthetic_cff() -> Vec<u8> {
        let mut v = b"CFF-TRANSLATOR-VERSION:02.01.03\nCFF:TEST\n".to_vec();
        v.resize(0x400, 0);
        v.extend_from_slice(&[0xED, 0x05, 0xEA, 0x07, 0x09, 0x0F]);
        v.resize(0x1000, 0xFF);
        v
    }

    #[tokio::test]
    async fn preflight_rejects_a_cff_container() {
        let rom = synthetic_cff();
        let mut iface = happy_ecu();
        let report = FlashingWorker::new()
            .run_preflight_checks(&manifest(&rom), &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(
            report.details.iter().any(|d| d.contains("flash container")),
            "{:?}",
            report.details
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cff_container_never_reaches_the_erase() {
        let rom = synthetic_cff();
        let iface = happy_ecu();
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, rom).await;
        assert!(matches!(res, Err(SterngateError::PreFlightCheckFailed(_))));
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
            "erase must not be sent for a container"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn erase_status_nonzero_aborts_after_erase() {
        // The ECU accepts the erase routine but reports a non-zero status: the
        // sectors may be half-erased, so nothing may be downloaded on top.
        let iface = ScriptedInterface::new()
            .rule(0x22, &[F192_FF, F192_CF])
            .rule_once(0x10, &[SESSION_EXT])
            .rule(0x10, &[SESSION_PROG])
            .rule_once(0x27, &[SEED])
            .rule(0x27, &[KEY_OK])
            .rule(0x28, &[COMM_OFF])
            .rule(0x85, &[DTC_OFF])
            .rule(0x31, &[&[0x05, 0x71, 0x01, 0xFF, 0x00, 0x01]])
            .rule(0x34, &[DOWNLOAD_OK]);
        let log = iface.sent_handle();
        let (flasher, res, _) = run(iface, vec![0x5A; 300]).await;
        assert!(matches!(res, Err(SterngateError::FlashAborted(_))));
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
        assert!(sids.contains(&0x31), "the erase routine was requested");
        assert!(
            !sids.contains(&0x34),
            "nothing may be downloaded over a failed erase"
        );
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

    #[tokio::test]
    async fn preflight_hw_mismatch_fails_closed() {
        let mut iface = ScriptedInterface::new().rule(
            0x22,
            &[
                &[0x10, 0x0D, 0x62, 0xF1, 0x92, b'0', b'2', b'8'],
                &[0x21, b'1', b'0', b'1', b'3', b'3', b'4', b'5'],
            ],
        );
        let rom = vec![0x5A; 64];
        let m = manifest(&rom); // expects 0281012224
        let report = FlashingWorker::new()
            .run_preflight_checks(&m, &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(!report.hw_id_match);
        assert!(!report.passed);
        assert!(report.details.iter().any(|d| d.contains("MISMATCH")));
    }

    #[tokio::test]
    async fn preflight_hw_read_error_fails_closed() {
        let mut iface = ScriptedInterface::new().rule(0x22, &[&[0x03, 0x7F, 0x22, 0x31]]);
        let rom = vec![0x5A; 64];
        let report = FlashingWorker::new()
            .run_preflight_checks(&manifest(&rom), &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(!report.hw_id_match);
    }

    #[tokio::test]
    async fn preflight_not_connected_fails_closed() {
        let mut iface = ScriptedInterface::new().disconnected();
        let rom = vec![0x5A; 64];
        let report = FlashingWorker::new()
            .run_preflight_checks(&manifest(&rom), &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(iface.sent_frames().is_empty());
    }

    #[tokio::test]
    async fn preflight_empty_manifest_hw_id_fails_closed() {
        // POST /api/v1/flash/stage deserialises the manifest from the request
        // body, so a package with no declared hardware id is reachable from
        // outside. It is unverifiable, not permissive: refuse without asking.
        let mut iface = ScriptedInterface::new().rule(0x22, &[F192_FF, F192_CF]);
        let rom = vec![0x5A; 64];
        let mut m = manifest(&rom);
        m.expected_hw_id = "  ".into();
        let report = FlashingWorker::new()
            .run_preflight_checks(&m, &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(!report.hw_id_match);
        assert!(!report.passed);
        assert!(
            iface.sent_frames().is_empty(),
            "an unusable manifest must not put a request on the bus"
        );
    }

    #[tokio::test]
    async fn preflight_sibling_variant_is_not_a_match() {
        // 0281012224 vs 0281012238: only the last digits differ; a prefix rule would accept it.
        let mut iface = ScriptedInterface::new().rule(
            0x22,
            &[
                &[0x10, 0x0D, 0x62, 0xF1, 0x92, b'0', b'2', b'8'],
                &[0x21, b'1', b'0', b'1', b'2', b'2', b'3', b'8'],
            ],
        );
        let rom = vec![0x5A; 64];
        let report = FlashingWorker::new()
            .run_preflight_checks(&manifest(&rom), &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(!report.hw_id_match);
    }

    #[tokio::test]
    async fn read_supplier_hw_id_renders_a_non_ascii_reply_as_hex() {
        // A binary-coded F192 must be rendered as its digits, not passed on as
        // the control characters those bytes spell in ASCII.
        let mut iface = ScriptedInterface::new()
            .rule(0x22, &[&[0x07, 0x62, 0xF1, 0x92, 0x02, 0x81, 0x01, 0x22]]);
        let live = read_supplier_hw_id(&mut iface, FLASH_TX_ID, FLASH_RX_ID)
            .await
            .unwrap();
        assert_eq!(live, "02810122");
    }

    #[tokio::test]
    async fn read_supplier_hw_id_rejects_a_reply_for_another_did() {
        // An ECU that answers F194 (the software number) to an F192 request must
        // not have its calibration id mistaken for a hardware id.
        let mut iface = ScriptedInterface::new()
            .rule(0x22, &[&[0x07, 0x62, 0xF1, 0x94, b'1', b'0', b'3', b'7']]);
        assert!(matches!(
            read_supplier_hw_id(&mut iface, FLASH_TX_ID, FLASH_RX_ID).await,
            Err(SterngateError::IsoTpError(_))
        ));
    }

    #[tokio::test]
    async fn supplier_hw_id_strips_padding() {
        // F192 is a fixed-length field: a real ECU returns "0281012224" plus
        // filler. 19 payload bytes (62 F1 92 + 10 digits + 6 pad) => FF + 2 CFs.
        for pad in [b' ', 0x00] {
            let mut iface = ScriptedInterface::new().rule(
                0x22,
                &[
                    &[0x10, 0x13, 0x62, 0xF1, 0x92, b'0', b'2', b'8'],
                    &[0x21, b'1', b'0', b'1', b'2', b'2', b'2', b'4'],
                    &[0x22, pad, pad, pad, pad, pad, pad, 0xAA],
                ],
            );
            let live = read_supplier_hw_id(&mut iface, FLASH_TX_ID, FLASH_RX_ID)
                .await
                .unwrap();
            assert_eq!(live, "0281012224", "padding byte {pad:#04X}");
        }
    }

    #[tokio::test]
    async fn read_supplier_hw_id_rejects_a_padding_only_reply() {
        // An erased or unprogrammed field is not an identity.
        let mut iface = ScriptedInterface::new()
            .rule(0x22, &[&[0x07, 0x62, 0xF1, 0x92, 0xFF, 0xFF, 0xFF, 0xFF]]);
        assert!(matches!(
            read_supplier_hw_id(&mut iface, FLASH_TX_ID, FLASH_RX_ID).await,
            Err(SterngateError::IsoTpError(_))
        ));
    }

    #[tokio::test]
    async fn preflight_accepts_a_space_padded_hw_id() {
        let mut iface = ScriptedInterface::new().rule(
            0x22,
            &[
                &[0x10, 0x13, 0x62, 0xF1, 0x92, b'0', b'2', b'8'],
                &[0x21, b'1', b'0', b'1', b'2', b'2', b'2', b'4'],
                &[0x22, b' ', b' ', b' ', b' ', b' ', b' ', 0xAA],
            ],
        );
        let rom = vec![0x5A; 64];
        let report = FlashingWorker::new()
            .run_preflight_checks(&manifest(&rom), &rom, 13.5, &mut iface)
            .await
            .unwrap();
        assert!(report.hw_id_match);
        assert!(report.passed);
    }

    #[test]
    fn hw_id_matches_is_exact() {
        assert!(hw_id_matches("0281012224", " 0281012224 "));
        assert!(!hw_id_matches("0281012224", "02810122"));
        assert!(!hw_id_matches("0281012224", "0281012238"));
        assert!(!hw_id_matches("", ""));
        assert!(!hw_id_matches("0281012224", ""));
    }

    #[tokio::test]
    async fn inspect_rom_connected_but_silent_is_not_flashable() {
        let mut iface = ScriptedInterface::new().rule(0x22, &[&[0x03, 0x7F, 0x22, 0x31]]);
        let mut rom = vec![0xEA; 4096];
        rom[64..74].copy_from_slice(b"0281012224");
        let report = FlashingWorker::new()
            .inspect_rom(&mut iface, 0x7E0, 0x7E8, &rom)
            .await
            .unwrap();
        assert!(!report.can_flash);
        assert_eq!(report.verdict, RomCompatibilityVerdict::Unknown);
    }

    // Paused time: nothing is scripted, so all three identification reads wait
    // out the full ISO-TP timeout; virtual time keeps that off the clock.
    #[tokio::test(start_paused = true)]
    async fn inspect_rom_without_markers_is_not_flashable() {
        let mut iface = ScriptedInterface::new();
        let report = FlashingWorker::new()
            .inspect_rom(&mut iface, 0x7E0, 0x7E8, &vec![0xEA; 4096])
            .await
            .unwrap();
        assert!(!report.can_flash);
    }
}
