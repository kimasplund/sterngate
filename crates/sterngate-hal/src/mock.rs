use crate::interface::VehicleInterface;
use async_trait::async_trait;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;
use sterngate_core::{CanFrame, Result};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct VirtualCanInterface {
    name: String,
    tx_queue: Sender<CanFrame>,
    rx_queue: Arc<Mutex<Receiver<CanFrame>>>,
    is_open: bool,
    start_time: Instant,
    dtc_cleared: Arc<AtomicBool>,
    last_multi_frame_sid: Arc<AtomicU8>,
    /// Block Sequence Counter from the most recent multi-frame `0x36` (TransferData) request.
    last_multi_frame_bsc: Arc<AtomicU8>,
    /// DID from the most recent multi-frame `0x2E` (WriteDataByIdentifier) request.
    last_multi_frame_did: Arc<AtomicU16>,
    expected_cfs: Arc<AtomicU32>,
    received_cfs: Arc<AtomicU32>,
    /// Consecutive Frames of a queued multi-frame ECU reply (e.g. an identification
    /// DID), released to `tx_queue` once the tester sends Flow Control.
    pending_cfs: Arc<Mutex<VecDeque<CanFrame>>>,
    /// UDS service IDs that answer NRC 0x31 instead of their normal reply.
    /// Test-only fault injection so fail-closed paths can be proven on CI.
    failing_services: HashSet<u8>,
}

impl VirtualCanInterface {
    pub fn new() -> Self {
        let (tx, rx) = channel::<CanFrame>(256);
        Self {
            name: "Virtual_W211_Simulator".to_string(),
            tx_queue: tx,
            rx_queue: Arc::new(Mutex::new(rx)),
            is_open: false,
            start_time: Instant::now(),
            dtc_cleared: Arc::new(AtomicBool::new(false)),
            last_multi_frame_sid: Arc::new(AtomicU8::new(0x2E)),
            last_multi_frame_bsc: Arc::new(AtomicU8::new(0)),
            last_multi_frame_did: Arc::new(AtomicU16::new(0)),
            expected_cfs: Arc::new(AtomicU32::new(1)),
            received_cfs: Arc::new(AtomicU32::new(0)),
            pending_cfs: Arc::new(Mutex::new(VecDeque::new())),
            failing_services: HashSet::new(),
        }
    }

    /// A virtual ECU that answers every request for the listed UDS service
    /// IDs with `7F <sid> 31` (RequestOutOfRange).
    pub fn with_failing_services(sids: &[u8]) -> Self {
        let mut sim = Self::new();
        sim.failing_services = sids.iter().copied().collect();
        sim
    }

    /// Queue a multi-frame ISO-TP reply: returns the First Frame now and parks
    /// the Consecutive Frames until the tester's Flow Control arrives.
    fn multi_frame_reply(&self, resp_id: u16, payload: &[u8]) -> CanFrame {
        let len = payload.len().min(0x0FFF);
        let mut ff = vec![
            0x10 | u8::try_from(len >> 8).unwrap_or(0x0F),
            u8::try_from(len & 0xFF).unwrap_or(0xFF),
        ];
        ff.extend_from_slice(&payload[..len.min(6)]);
        let mut sn = 1u8;
        let mut cfs = VecDeque::new();
        for chunk in payload[len.min(6)..len].chunks(7) {
            let mut cf = vec![0x20 | (sn & 0x0F)];
            cf.extend_from_slice(chunk);
            while cf.len() < 8 {
                cf.push(0xAA);
            }
            cfs.push_back(CanFrame::new_standard(resp_id, &cf));
            sn = sn.wrapping_add(1) & 0x0F;
        }
        if let Ok(mut q) = self.pending_cfs.try_lock() {
            *q = cfs;
        }
        CanFrame::new_standard(resp_id, &ff)
    }

    fn generate_telemetry_frame(&self, req_id: u32, payload: &[u8]) -> Option<CanFrame> {
        let resp_id = match req_id {
            0x7DF => 0x7E8,
            id if (0x700..=0x7EF).contains(&id) => id + 8,
            _ => 0x7E8,
        };

        if payload.is_empty() {
            return None;
        }

        let pci = payload[0];
        let pci_type = pci >> 4;

        if pci_type == 1 {
            // ISO-TP First Frame: Send Flow Control (0x30: ContinueToSend)
            let total_len = (((payload[0] as usize) & 0x0F) << 8) | (payload[1] as usize);
            let needed_cfs = if total_len > 6 {
                (total_len - 6).div_ceil(7)
            } else {
                1
            };
            self.expected_cfs
                .store(needed_cfs as u32, Ordering::Relaxed);
            self.received_cfs.store(0, Ordering::Relaxed);
            if payload.len() >= 3 {
                self.last_multi_frame_sid
                    .store(payload[2], Ordering::Relaxed);
            }
            if payload.len() >= 5 && payload[2] == 0x2E {
                self.last_multi_frame_did.store(
                    u16::from_be_bytes([payload[3], payload[4]]),
                    Ordering::Relaxed,
                );
            }
            if payload.len() >= 4 && payload[2] == 0x36 {
                self.last_multi_frame_bsc
                    .store(payload[3], Ordering::Relaxed);
            }
            return Some(CanFrame::new_standard(
                resp_id as u16,
                &[0x30, 0x00, 0x00, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA],
            ));
        }

        if pci_type == 2 {
            let received = self.received_cfs.fetch_add(1, Ordering::Relaxed) + 1;
            let expected = self.expected_cfs.load(Ordering::Relaxed);
            if received < expected {
                return None;
            }
            // ISO-TP Consecutive Frame: acknowledge completion once all frames received
            let sid = self.last_multi_frame_sid.load(Ordering::Relaxed);
            if self.failing_services.contains(&sid) {
                return Some(CanFrame::new_standard(
                    resp_id as u16,
                    &[0x03, 0x7F, sid, 0x31],
                ));
            }
            let resp_bytes = match sid {
                0x3D => vec![0x02, 0x7D, 0x24, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA],
                0x23 => {
                    // An 8-byte 0x23 request puts [SID, ALFID, addr x4] in the
                    // First Frame; the single CF carries [len_hi, len_lo].
                    let n = if payload.len() >= 3 {
                        usize::from(u16::from_be_bytes([payload[1], payload[2]]))
                    } else {
                        0
                    };
                    if n == 0 || n > 6 {
                        // Only single-frame replies are emitted by this mock.
                        vec![0x03, 0x7F, 0x23, 0x31]
                    } else {
                        let mut r = vec![u8::try_from(1 + n).unwrap_or(0x07), 0x63];
                        r.resize(2 + n, 0x00);
                        r
                    }
                }
                0x34 => vec![0x04, 0x74, 0x20, 0x0F, 0xFF],
                0x36 => vec![
                    0x02,
                    0x76,
                    self.last_multi_frame_bsc.load(Ordering::Relaxed),
                ],
                0x2E => {
                    let did = self
                        .last_multi_frame_did
                        .load(Ordering::Relaxed)
                        .to_be_bytes();
                    vec![0x03, 0x6E, did[0], did[1]]
                }
                _ => vec![0x03, 0x7F, sid, 0x11],
            };
            return Some(CanFrame::new_standard(resp_id as u16, &resp_bytes));
        }

        let service = if pci_type == 0 {
            if payload.len() > 1 {
                payload[1]
            } else {
                return None;
            }
        } else {
            payload[0]
        };

        if self.failing_services.contains(&service) {
            return Some(CanFrame::new_standard(
                resp_id as u16,
                &[0x03, 0x7F, service, 0x31],
            ));
        }

        let elapsed = self.start_time.elapsed().as_secs_f64();
        let rpm_var = (elapsed.sin() * 50.0) as i16;
        let base_rpm: u16 = (820 + rpm_var).max(700) as u16;
        let rpm_raw = base_rpm * 4;

        match service {
            // DiagnosticSessionControl
            0x10 => {
                let sub = if payload.len() > 2 { payload[2] } else { 0x01 };
                Some(CanFrame::new_standard(
                    resp_id as u16,
                    &[0x06, 0x50, sub, 0x00, 0x32, 0x01, 0xF4],
                ))
            }
            // ECUReset (0x11)
            0x11 => {
                let sub = if payload.len() > 2 { payload[2] } else { 0x01 };
                Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x51, sub]))
            }
            // SecurityAccess
            0x27 => {
                let sub = if payload.len() > 2 { payload[2] } else { 0x01 };
                if sub % 2 == 1 {
                    Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x06, 0x67, sub, 0x12, 0x34, 0x56, 0x78],
                    ))
                } else {
                    Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x67, sub]))
                }
            }
            // TesterPresent
            0x3E => {
                if payload.get(2).is_some_and(|sub| sub & 0x80 != 0) {
                    None
                } else {
                    Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x7E, 0x00]))
                }
            }
            // RequestTransferExit
            0x37 => Some(CanFrame::new_standard(resp_id as u16, &[0x01, 0x77])),
            // CommunicationControl
            0x28 => {
                let sub = payload.get(2).copied().unwrap_or(0x01);
                Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x68, sub]))
            }
            // ControlDTCSetting
            0x85 => {
                let sub = payload.get(2).copied().unwrap_or(0x02);
                Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0xC5, sub]))
            }
            // RequestDownload (single-frame form)
            0x34 => Some(CanFrame::new_standard(
                resp_id as u16,
                &[0x04, 0x74, 0x20, 0x0F, 0xFF],
            )),
            // ReadDataByIdentifier
            0x22 => {
                if payload.len() < 4 {
                    return None;
                }
                let did = u16::from_be_bytes([payload[2], payload[3]]);
                match did {
                    // Engine RPM (0x0100)
                    0x0100 => {
                        let b = rpm_raw.to_be_bytes();
                        Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x05, 0x62, 0x01, 0x00, b[0], b[1], 0xAA, 0xAA],
                        ))
                    }
                    // Coolant Temp (0x0105) -> 88°C (88 + 40 = 128 = 0x80)
                    0x0105 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x04, 0x62, 0x01, 0x05, 0x80, 0xAA, 0xAA, 0xAA],
                    )),
                    // Rail Pressure (0x200B) -> 320.0 bar (3200 = 0x0C80)
                    0x200B => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x05, 0x62, 0x20, 0x0B, 0x0C, 0x80, 0xAA, 0xAA],
                    )),
                    // Boost Pressure (0x2010) -> 1040 hPa (0x0410)
                    0x2010 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x05, 0x62, 0x20, 0x10, 0x04, 0x10, 0xAA, 0xAA],
                    )),
                    // Injector Smooth Running (0x2021..0x2024)
                    0x2021 => {
                        // Cyl 1: +0.22 mm³ -> raw = (0.22 + 5.0)/0.01 = 522 = 0x020A
                        Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x05, 0x62, 0x20, 0x21, 0x02, 0x0A, 0xAA, 0xAA],
                        ))
                    }
                    0x2022 => {
                        // Cyl 2: -0.15 mm³ -> raw = (-0.15 + 5.0)/0.01 = 485 = 0x01E5
                        Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x05, 0x62, 0x20, 0x22, 0x01, 0xE5, 0xAA, 0xAA],
                        ))
                    }
                    0x2023 => {
                        // Cyl 3: -0.32 mm³ -> raw = (-0.32 + 5.0)/0.01 = 468 = 0x01D4
                        Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x05, 0x62, 0x20, 0x23, 0x01, 0xD4, 0xAA, 0xAA],
                        ))
                    }
                    0x2024 => {
                        // Cyl 4: +0.25 mm³ -> raw = (0.25 + 5.0)/0.01 = 525 = 0x020D
                        Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x05, 0x62, 0x20, 0x24, 0x02, 0x0D, 0xAA, 0xAA],
                        ))
                    }
                    // 722.6 Transmission Fluid Temp (0x2001) -> 80°C (80 + 40 = 120 = 0x78)
                    0x2001 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x04, 0x62, 0x20, 0x01, 0x78, 0xAA, 0xAA, 0xAA],
                    )),
                    // 722.6 TCC Slip (0x2002) -> 16 RPM
                    0x2002 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x05, 0x62, 0x20, 0x02, 0x00, 0x10, 0xAA, 0xAA],
                    )),
                    // 722.6 N2 Turbine Speed (0x2003) -> 820 RPM
                    0x2003 => {
                        let b = base_rpm.to_be_bytes();
                        Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x05, 0x62, 0x20, 0x03, b[0], b[1], 0xAA, 0xAA],
                        ))
                    }
                    // Airmatic Ride Height (0x2050) -> 2450 mV
                    0x2050 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x05, 0x62, 0x20, 0x50, 0x09, 0x92, 0xAA, 0xAA],
                    )),
                    // Airmatic Pressure (0x2051) -> 14.2 bar (142 = 0x8E)
                    0x2051 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x04, 0x62, 0x20, 0x51, 0x8E, 0xAA, 0xAA, 0xAA],
                    )),
                    // Identification DIDs
                    // Spare Part Number (0xF187) -> "A6461500879" (first 4 bytes)
                    0xF187 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0xF1, 0x87, b'6', b'4', b'6', b'1'],
                    )),
                    // Software Application / Calibration (0xF188)
                    0xF188 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0xF1, 0x88, 0x10, 0x37, 0x38, 0x66],
                    )),
                    // Software Version (0xF189)
                    0xF189 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0xF1, 0x89, 0x00, 0x24, 0x48, 0x33],
                    )),
                    // VIN (0xF190) -> "WDB2"
                    0xF190 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0xF1, 0x90, b'W', b'D', b'B', b'2'],
                    )),
                    // VIN Current (0xF1A0) -> "WDB2"
                    0xF1A0 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0xF1, 0xA0, b'W', b'D', b'B', b'2'],
                    )),
                    // Hardware Version (0xF191)
                    0xF191 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0xF1, 0x91, 0x00, 0x01, 0x53, 0x54],
                    )),
                    // System Supplier Hardware Number (0xF192) -> Bosch HW 0281012224
                    // (multi-frame ASCII; the flasher's preflight reads this DID)
                    0xF192 => {
                        Some(self.multi_frame_reply(resp_id as u16, b"\x62\xF1\x920281012224"))
                    }
                    // System Supplier Software Number (0xF194) -> Bosch SW 1037372332
                    0xF194 => {
                        Some(self.multi_frame_reply(resp_id as u16, b"\x62\xF1\x941037372332"))
                    }
                    // System Name (0xF197) -> "CR4 "
                    0xF197 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0xF1, 0x97, b'C', b'R', b'4', b' '],
                    )),
                    // Common Rail Injector IMA Calibration Codes (0x2030..0x2033)
                    0x2030 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0x20, 0x30, b'7', b'B', b'8', b'H'],
                    )),
                    0x2031 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0x20, 0x31, b'A', b'8', b'B', b'1'],
                    )),
                    0x2032 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0x20, 0x32, b'9', b'K', b'4', b'M'],
                    )),
                    0x2033 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x07, 0x62, 0x20, 0x33, b'5', b'J', b'7', b'T'],
                    )),
                    // ECO Start-Stop Memory Mode (0x0320) -> 0x01 (Remember last state)
                    0x0320 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x04, 0x62, 0x03, 0x20, 0x01, 0xAA, 0xAA, 0xAA],
                    )),
                    // EGR Adaptation Air Mass Offset (0x0240) -> +40 mg (0x0190)
                    0x0240 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x05, 0x62, 0x02, 0x40, 0x01, 0x90, 0xAA, 0xAA],
                    )),
                    // Speed Limiter VMax (0x0110) -> 250 km/h (0x00, 0xFA)
                    0x0110 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x05, 0x62, 0x01, 0x10, 0x00, 0xFA, 0xAA, 0xAA],
                    )),
                    // Seatbelt Acoustic Warning Chime (0x0201) -> 0x01 (Enabled)
                    0x0201 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x04, 0x62, 0x02, 0x01, 0x01, 0xAA, 0xAA, 0xAA],
                    )),
                    // Remaining Fuel Exact Liters (0x0205) -> 0x01 (Enabled)
                    0x0205 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x04, 0x62, 0x02, 0x05, 0x01, 0xAA, 0xAA, 0xAA],
                    )),
                    // Intelligent Cornering Fog Lights (0x0310) -> 0x01 (Enabled)
                    0x0310 => Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x04, 0x62, 0x03, 0x10, 0x01, 0xAA, 0xAA, 0xAA],
                    )),
                    _ => {
                        Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x03, 0x7F, 0x22, 0x31],
                        )) // RequestOutOfRange
                    }
                }
            }
            // ReadDTC (0x19)
            0x19 => {
                if self.dtc_cleared.load(Ordering::Relaxed) {
                    Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x59, 0x02]))
                } else {
                    // P0100 (MAF circuit malfunction)
                    Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x06, 0x59, 0x02, 0x2F, 0x01, 0x00, 0x28],
                    ))
                }
            }
            // ClearDTC (0x14)
            0x14 => {
                self.dtc_cleared.store(true, Ordering::Relaxed);
                Some(CanFrame::new_standard(resp_id as u16, &[0x01, 0x54]))
            }
            // WriteDataByIdentifier (0x2E)
            0x2E => {
                if payload.len() >= 4 {
                    Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x03, 0x6E, payload[2], payload[3]],
                    ))
                } else {
                    Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x03, 0x7F, 0x2E, 0x13],
                    ))
                }
            }
            // RoutineControl (0x31)
            0x31 => {
                if payload.len() >= 5 {
                    let sub_fn = payload[2];
                    let r_hi = payload[3];
                    let r_lo = payload[4];
                    let routine_id = u16::from_be_bytes([r_hi, r_lo]);
                    match routine_id {
                        // 0xFF01: Fuel Pump Prime & Rail Bleed
                        // 0x0201: Reset Zero-Quantity Injector Adaptations (NMK)
                        // 0x0202: DPF Regeneration Trigger
                        // 0x0203: Throttle Valve / EGR Lower Stop Relearn
                        // 0x0205: SBC Brake Hydraulic Bleeding Routine
                        // 0x0206: SBC Brake Pad Service Deactivation (0 bar, finger safety)
                        // 0x0207: SBC Brake System Reactivation & Bleed
                        // 0x0210: Compressor Relay Force Inhibit (Burnout Safe Mode)
                        // 0x0211: Suspension Workshop / Transport Mode (Leveling Inhibit)
                        // 0x0212: Suspension Normal Operation Restore
                        // 0x0213: Suspension Corner Inflate
                        // 0x0214: Suspension Corner Deflate
                        // 0x0215: Suspension Zero-Level Sensor Calibration
                        // 0x0220: ABC System Pressure Fallback Dump (120 bar)
                        // 0x0221: ABC Strut Isolation Valve Lock
                        // 0x0222: ABC Normal Active Suspension Restore
                        // 0x0218: Reset SCR Warning & Start Lockout Counter (AdBlue 800km countdown)
                        // 0x0219: Reset SCR Catalyst & NOx Quality Adaptation
                        // 0x021A: AdBlue Tank Level Ultrasonic Re-teach
                        0xFF01 | 0x0201 | 0x0202 | 0x0203 | 0x0205 | 0x0206 | 0x0207 | 0x0210
                        | 0x0211 | 0x0212 | 0x0213 | 0x0214 | 0x0215 | 0x0218 | 0x0219 | 0x021A
                        | 0x0220 | 0x0221 | 0x0222 | 0xFF00 => Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x05, 0x71, sub_fn, r_hi, r_lo, 0x00],
                        )),
                        _ => Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x05, 0x71, sub_fn, r_hi, r_lo, 0x00],
                        )),
                    }
                } else {
                    Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x03, 0x7F, 0x31, 0x13], // IncorrectMessageLength
                    ))
                }
            }
            // WriteMemoryByAddress (0x3D)
            0x3D => Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x7D, 0x24])),
            // ReadMemoryByAddress (0x23)
            0x23 => Some(CanFrame::new_standard(
                resp_id as u16,
                &[0x04, 0x63, 0x00, 0x00, 0x00],
            )),
            _ => {
                Some(CanFrame::new_standard(
                    resp_id as u16,
                    &[0x03, 0x7F, service, 0x11],
                )) // ServiceNotSupported
            }
        }
    }
}

impl Default for VirtualCanInterface {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VehicleInterface for VirtualCanInterface {
    async fn open(&mut self) -> Result<()> {
        self.is_open = true;
        Ok(())
    }

    async fn send(&mut self, frame: CanFrame) -> Result<()> {
        if !self.is_open {
            return Err(sterngate_core::SterngateError::DeviceNotFound(
                "Virtual CAN not open".into(),
            ));
        }

        if frame.data.first().is_some_and(|b| b >> 4 == 0x3) {
            // Tester Flow Control: release any parked Consecutive Frames from a
            // queued multi-frame ECU reply instead of generating a new reply.
            let mut pending = self.pending_cfs.lock().await;
            while let Some(cf) = pending.pop_front() {
                let _ = self.tx_queue.send(cf).await;
            }
            return Ok(());
        }

        if let Some(resp) = self.generate_telemetry_frame(frame.id, &frame.data) {
            let _ = self.tx_queue.send(resp).await;
        }

        Ok(())
    }

    async fn recv(&mut self) -> Result<CanFrame> {
        if !self.is_open {
            return Err(sterngate_core::SterngateError::DeviceNotFound(
                "Virtual CAN not open".into(),
            ));
        }

        let mut rx = self.rx_queue.lock().await;
        tokio::select! {
            frame = rx.recv() => {
                frame.ok_or_else(|| sterngate_core::SterngateError::HalError("Mock queue closed".into()))
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {
                // Background broadcast: Engine RPM broadcast frame
                let elapsed = self.start_time.elapsed().as_secs_f64();
                let rpm_var = (elapsed.sin() * 50.0) as i16;
                let base_rpm: u16 = (820 + rpm_var).max(700) as u16;
                let b = (base_rpm * 4).to_be_bytes();
                Ok(CanFrame::new_standard(0x308, &[b[0], b[1], 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]))
            }
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.is_open = false;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_connected(&self) -> bool {
        self.is_open
    }
}
