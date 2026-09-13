use crate::interface::VehicleInterface;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
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
        }
    }

    fn generate_telemetry_frame(&self, req_id: u32, payload: &[u8]) -> Option<CanFrame> {
        let resp_id = match req_id {
            0x7DF | 0x7E0 => 0x7E8,
            0x7E1 => 0x7E9,
            0x7E4 => 0x7EC,
            0x7E6 => 0x7EE,
            _ => 0x7E8,
        };

        if payload.is_empty() {
            return None;
        }

        let pci = payload[0];
        let pci_type = pci >> 4;

        let service = if pci_type == 0 {
            if payload.len() > 1 {
                payload[1]
            } else {
                return None;
            }
        } else {
            payload[0]
        };

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
            // SecurityAccess
            0x27 => {
                let sub = if payload.len() > 2 { payload[2] } else { 0x01 };
                if sub == 0x01 || sub == 0x03 || sub == 0x0B {
                    Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x06, 0x67, sub, 0x12, 0x34, 0x56, 0x78],
                    ))
                } else {
                    Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x67, sub]))
                }
            }
            // TesterPresent
            0x3E => Some(CanFrame::new_standard(resp_id as u16, &[0x02, 0x7E, 0x80])),
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
                        // 0x0210: Compressor Relay Force Inhibit (Burnout Safe Mode)
                        // 0x0211: Suspension Workshop / Transport Mode (Leveling Inhibit)
                        // 0x0212: Suspension Normal Operation Restore
                        // 0x0220: ABC System Pressure Fallback Dump (120 bar)
                        // 0x0221: ABC Strut Isolation Valve Lock
                        // 0x0222: ABC Normal Active Suspension Restore
                        // 0xFF00: Erase Flash Routine
                        0xFF01 | 0x0201 | 0x0202 | 0x0203 | 0x0205 | 0x0210 | 0x0211 | 0x0212
                        | 0x0220 | 0x0221 | 0x0222 | 0xFF00 => Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x05, 0x71, sub_fn, r_hi, r_lo, 0x00],
                        )),
                        _ => Some(CanFrame::new_standard(
                            resp_id as u16,
                            &[0x03, 0x7F, 0x31, 0x31], // RequestOutOfRange
                        )),
                    }
                } else {
                    Some(CanFrame::new_standard(
                        resp_id as u16,
                        &[0x03, 0x7F, 0x31, 0x13], // IncorrectMessageLength
                    ))
                }
            }
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
