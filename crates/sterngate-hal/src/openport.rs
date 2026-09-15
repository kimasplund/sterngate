//! Tactrix OpenPort 2.0 native Linux vehicle interface.
//!
//! Provides direct USB bulk communication with OpenPort 2.0 hardware
//! without proprietary Windows drivers, wine, or closed-source DLLs.
//! Includes built-in hardware battery voltage telemetry (Pin 16 ADC)
//! to enforce safe flashing interlocks.

use crate::interface::VehicleInterface;
use crate::openport_codec::{
    OpenPortCommand, OpenPortDecoder, OpenPortResponse, CHANNEL_CAN, TACTRIX_PRODUCT_ID_BOOTLOADER,
    TACTRIX_PRODUCT_ID_COMPOSITE, TACTRIX_PRODUCT_ID_OP20, TACTRIX_VENDOR_ID,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use sterngate_core::{CanFrame, Result, SterngateError};
use tokio::sync::{mpsc, Mutex};

/// Operational mode for the OpenPort interface.
enum OpenPortBackend {
    /// Physical USB device using `rusb`.
    Hardware {
        handle: Arc<Mutex<rusb::DeviceHandle<rusb::GlobalContext>>>,
        endpoint_out: u8,
        endpoint_in: u8,
        interface_num: u8,
    },
    /// In-memory simulation for testing and offline environments.
    Simulated {
        sim_voltage_mv: u32,
        rx_tx: mpsc::Sender<CanFrame>,
    },
}

/// Tactrix OpenPort 2.0 hardware interface.
pub struct OpenPortInterface {
    name: String,
    baud: u32,
    is_open: Arc<AtomicBool>,
    backend: Option<OpenPortBackend>,
    rx_channel: Option<mpsc::Receiver<CanFrame>>,
    rx_sender: Option<mpsc::Sender<CanFrame>>,
    device_info: Option<String>,
    last_voltage_v: Option<f32>,
}

impl OpenPortInterface {
    /// Create a new OpenPort interface targeting high-speed CAN (default 500 kbps).
    pub fn new() -> Self {
        Self::with_baud(500_000)
    }

    /// Create an OpenPort interface with a custom baudrate (e.g. 500000, 250000, 125000).
    pub fn with_baud(baud: u32) -> Self {
        let (tx, rx) = mpsc::channel(256);
        Self {
            name: "Tactrix OpenPort 2.0 (Native USB)".to_string(),
            baud,
            is_open: Arc::new(AtomicBool::new(false)),
            backend: None,
            rx_channel: Some(rx),
            rx_sender: Some(tx),
            device_info: None,
            last_voltage_v: None,
        }
    }

    /// Create a simulated OpenPort interface for tests and virtual offline simulation.
    pub fn new_simulated(initial_voltage_v: f32) -> (Self, mpsc::Sender<CanFrame>) {
        let (driver_tx, driver_rx) = mpsc::channel(256);
        let (sim_feed_tx, mut sim_feed_rx) = mpsc::channel(256);

        let driver_tx_clone = driver_tx.clone();
        tokio::spawn(async move {
            while let Some(frame) = sim_feed_rx.recv().await {
                let _ = driver_tx_clone.send(frame).await;
            }
        });

        let iface = Self {
            name: "Tactrix OpenPort 2.0 (Simulated Loopback)".to_string(),
            baud: 500_000,
            is_open: Arc::new(AtomicBool::new(false)),
            backend: Some(OpenPortBackend::Simulated {
                sim_voltage_mv: (initial_voltage_v * 1000.0) as u32,
                rx_tx: driver_tx,
            }),
            rx_channel: Some(driver_rx),
            rx_sender: None,
            device_info: Some("Tactrix OpenPort 2.0 (Sterngate Emulated v1.0)".to_string()),
            last_voltage_v: Some(initial_voltage_v),
        };

        (iface, sim_feed_tx)
    }

    /// Read vehicle battery voltage directly from OpenPort Pin 16 ADC in Volts.
    ///
    /// Essential for verifying the safe flashing voltage interlock (>= 12.5V).
    pub async fn read_battery_voltage(&mut self) -> Result<f32> {
        if !self.is_connected() {
            return Err(SterngateError::DeviceNotFound(
                "OpenPort device is not open".into(),
            ));
        }

        match self.backend.as_ref() {
            Some(OpenPortBackend::Hardware {
                handle,
                endpoint_out,
                endpoint_in,
                ..
            }) => {
                let cmd = OpenPortCommand::ReadPinVoltage { pin: 16 }.encode();
                let handle_guard = handle.lock().await;

                // Send atr 16
                handle_guard
                    .write_bulk(*endpoint_out, &cmd, Duration::from_millis(500))
                    .map_err(|e| {
                        SterngateError::HalError(format!(
                            "Failed to write voltage query to OpenPort: {}",
                            e
                        ))
                    })?;

                // Read arr 16 <mV> response
                let mut in_buf = vec![0u8; 128];
                let bytes_read = handle_guard
                    .read_bulk(*endpoint_in, &mut in_buf, Duration::from_millis(500))
                    .map_err(|e| {
                        SterngateError::HalError(format!(
                            "Failed to read voltage response from OpenPort: {}",
                            e
                        ))
                    })?;

                let mut decoder = OpenPortDecoder::new();
                decoder.feed(&in_buf[..bytes_read]);

                while let Some(resp) = decoder.next_response() {
                    if let OpenPortResponse::PinVoltage {
                        pin: 16,
                        millivolts,
                    } = resp
                    {
                        let volts = millivolts as f32 / 1000.0;
                        self.last_voltage_v = Some(volts);
                        return Ok(volts);
                    }
                }

                Err(SterngateError::ProtocolError(
                    "OpenPort returned unexpected response to voltage query".into(),
                ))
            }
            Some(OpenPortBackend::Simulated { sim_voltage_mv, .. }) => {
                let volts = *sim_voltage_mv as f32 / 1000.0;
                self.last_voltage_v = Some(volts);
                Ok(volts)
            }
            None => Err(SterngateError::DeviceNotFound(
                "OpenPort backend not initialized".into(),
            )),
        }
    }

    /// Retrieve hardware device and firmware version string.
    pub fn device_info(&self) -> Option<&str> {
        self.device_info.as_deref()
    }

    /// Synchronous helper to discover, open, and configure the OpenPort 2.0 USB device.
    /// Kept synchronous so non-Send libusb device pointers are not held across await points.
    fn find_and_open_device() -> Result<(rusb::DeviceHandle<rusb::GlobalContext>, u8, u8, u8)> {
        tracing::info!("Scanning USB bus for Tactrix OpenPort 2.0 (VID 0x0403)...");

        let devices = rusb::devices()
            .map_err(|e| SterngateError::HalError(format!("Failed to list USB devices: {}", e)))?;

        let mut target_device = None;

        for dev in devices.iter() {
            if let Ok(desc) = dev.device_descriptor() {
                if desc.vendor_id() == TACTRIX_VENDOR_ID {
                    let pid = desc.product_id();
                    if pid == TACTRIX_PRODUCT_ID_OP20 || pid == TACTRIX_PRODUCT_ID_COMPOSITE {
                        target_device = Some(dev);
                        break;
                    } else if pid == TACTRIX_PRODUCT_ID_BOOTLOADER {
                        return Err(SterngateError::HalError(
                            "Tactrix OpenPort 2.0 is in bootloader mode (0403:cc4b). Reconnect device."
                                .into(),
                        ));
                    }
                }
            }
        }

        let device = target_device.ok_or_else(|| {
            SterngateError::DeviceNotFound(
                "Tactrix OpenPort 2.0 hardware (0403:cc4d/cc4c) not found on USB bus. Ensure device is plugged in and udev rules are loaded.".into(),
            )
        })?;

        let handle = device.open().map_err(|e| {
            SterngateError::HalError(format!(
                "Failed to open Tactrix OpenPort 2.0 USB device handle: {}. Check udev permissions (MODE=\"0666\").",
                e
            ))
        })?;

        let config_desc = device.active_config_descriptor().map_err(|e| {
            SterngateError::HalError(format!("Failed to read USB active config: {}", e))
        })?;

        let mut ep_in = None;
        let mut ep_out = None;
        let mut interface_num = 0;

        for interface in config_desc.interfaces() {
            for iface_desc in interface.descriptors() {
                let mut found_in = None;
                let mut found_out = None;
                for ep in iface_desc.endpoint_descriptors() {
                    if ep.transfer_type() == rusb::TransferType::Bulk {
                        if ep.direction() == rusb::Direction::In {
                            found_in = Some(ep.address());
                        } else if ep.direction() == rusb::Direction::Out {
                            found_out = Some(ep.address());
                        }
                    }
                }
                if let (Some(i), Some(o)) = (found_in, found_out) {
                    ep_in = Some(i);
                    ep_out = Some(o);
                    interface_num = iface_desc.interface_number();
                    break;
                }
            }
            if ep_in.is_some() {
                break;
            }
        }

        let (endpoint_in, endpoint_out) = match (ep_in, ep_out) {
            (Some(i), Some(o)) => (i, o),
            _ => {
                return Err(SterngateError::HalError(
                    "Tactrix OpenPort 2.0 bulk IN/OUT endpoints not found".into(),
                ));
            }
        };

        // Detach kernel driver if bound
        if handle.kernel_driver_active(interface_num).unwrap_or(false) {
            let _ = handle.detach_kernel_driver(interface_num);
        }

        Ok((handle, endpoint_in, endpoint_out, interface_num))
    }

    /// Retrieve last cached battery voltage measurement in Volts.
    pub fn last_voltage(&self) -> Option<f32> {
        self.last_voltage_v
    }
}

impl Default for OpenPortInterface {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VehicleInterface for OpenPortInterface {
    async fn open(&mut self) -> Result<()> {
        if self.is_open.load(Ordering::SeqCst) {
            return Ok(());
        }

        // If in simulated mode, mark open and return immediately
        if matches!(self.backend, Some(OpenPortBackend::Simulated { .. })) {
            self.is_open.store(true, Ordering::SeqCst);
            tracing::info!("OpenPort simulated interface active");
            return Ok(());
        }

        let (handle, endpoint_in, endpoint_out, interface_num) = Self::find_and_open_device()?;

        // Claim interface
        handle.claim_interface(interface_num).map_err(|e| {
            SterngateError::HalError(format!(
                "Failed to claim OpenPort USB interface {}: {}",
                interface_num, e
            ))
        })?;

        let timeout = Duration::from_millis(1500);

        // 1. Identify device: ati
        let identify_cmd = OpenPortCommand::Identify.encode();
        handle
            .write_bulk(endpoint_out, &identify_cmd, timeout)
            .map_err(|e| SterngateError::HalError(format!("Failed to write 'ati' query: {}", e)))?;

        let mut read_buf = vec![0u8; 128];
        let bytes_read = handle
            .read_bulk(endpoint_in, &mut read_buf, timeout)
            .map_err(|e| {
                SterngateError::HalError(format!("Failed to read 'ari' response: {}", e))
            })?;

        let mut decoder = OpenPortDecoder::new();
        decoder.feed(&read_buf[..bytes_read]);

        if let Some(OpenPortResponse::DeviceInfo(info)) = decoder.next_response() {
            tracing::info!("Tactrix OpenPort connected: {}", info);
            self.device_info = Some(info);
        }

        // 2. Activate device: ata
        let activate_cmd = OpenPortCommand::Activate.encode();
        handle
            .write_bulk(endpoint_out, &activate_cmd, timeout)
            .map_err(|e| {
                SterngateError::HalError(format!("Failed to write 'ata' activate: {}", e))
            })?;

        let bytes_read = handle
            .read_bulk(endpoint_in, &mut read_buf, timeout)
            .map_err(|e| {
                SterngateError::HalError(format!("Failed to read 'aro' activate: {}", e))
            })?;
        decoder.clear();
        decoder.feed(&read_buf[..bytes_read]);

        // 3. Open CAN channel: ato5 0 <baud> 0
        let open_ch_cmd = OpenPortCommand::OpenChannel {
            channel: CHANNEL_CAN,
            flags: 0,
            baud: self.baud,
        }
        .encode();
        handle
            .write_bulk(endpoint_out, &open_ch_cmd, timeout)
            .map_err(|e| SterngateError::HalError(format!("Failed to open CAN channel: {}", e)))?;

        let bytes_read = handle
            .read_bulk(endpoint_in, &mut read_buf, timeout)
            .map_err(|e| {
                SterngateError::HalError(format!("Failed to read channel 'aro': {}", e))
            })?;
        decoder.clear();
        decoder.feed(&read_buf[..bytes_read]);

        tracing::info!(
            "OpenPort CAN channel {} opened at {} bps",
            CHANNEL_CAN,
            self.baud
        );

        let handle_arc = Arc::new(Mutex::new(handle));
        self.backend = Some(OpenPortBackend::Hardware {
            handle: handle_arc.clone(),
            endpoint_out,
            endpoint_in,
            interface_num,
        });

        self.is_open.store(true, Ordering::SeqCst);

        // Spawn background reader thread to receive bulk frames continuously
        if let Some(tx) = self.rx_sender.take() {
            let is_open_flag = self.is_open.clone();
            tokio::spawn(async move {
                let mut decoder = OpenPortDecoder::new();
                let mut buf = vec![0u8; 1024];

                while is_open_flag.load(Ordering::SeqCst) {
                    let res = {
                        let guard = handle_arc.lock().await;
                        guard.read_bulk(endpoint_in, &mut buf, Duration::from_millis(20))
                    };

                    match res {
                        Ok(bytes_read) if bytes_read > 0 => {
                            decoder.feed(&buf[..bytes_read]);
                            while let Some(resp) = decoder.next_response() {
                                if let OpenPortResponse::ReceivedCan { frame, .. } = resp {
                                    if tx.send(frame).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(rusb::Error::Timeout) => {
                            // Normal timeout on idle CAN bus
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                        Err(e) => {
                            tracing::warn!("OpenPort bulk read error: {}", e);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                }
            });
        }

        // Measure initial pin 16 battery voltage
        let _ = self.read_battery_voltage().await;

        Ok(())
    }

    async fn send(&mut self, frame: CanFrame) -> Result<()> {
        if !self.is_connected() {
            return Err(SterngateError::DeviceNotFound(
                "OpenPort device not open".into(),
            ));
        }

        match self.backend.as_ref() {
            Some(OpenPortBackend::Hardware {
                handle,
                endpoint_out,
                ..
            }) => {
                let cmd = OpenPortCommand::TransmitCan {
                    channel: CHANNEL_CAN,
                    frame,
                }
                .encode();

                let handle_guard = handle.lock().await;
                handle_guard
                    .write_bulk(*endpoint_out, &cmd, Duration::from_millis(500))
                    .map_err(|e| {
                        SterngateError::HalError(format!(
                            "Failed to transmit frame to OpenPort: {}",
                            e
                        ))
                    })?;
                Ok(())
            }
            Some(OpenPortBackend::Simulated { rx_tx, .. }) => {
                // In simulation, loopback or forward frame
                let _ = rx_tx.send(frame).await;
                Ok(())
            }
            None => Err(SterngateError::DeviceNotFound(
                "OpenPort device not initialized".into(),
            )),
        }
    }

    async fn recv(&mut self) -> Result<CanFrame> {
        if !self.is_connected() {
            return Err(SterngateError::DeviceNotFound(
                "OpenPort device not open".into(),
            ));
        }

        if let Some(rx) = self.rx_channel.as_mut() {
            match rx.recv().await {
                Some(frame) => Ok(frame),
                None => Err(SterngateError::HalError("OpenPort RX stream closed".into())),
            }
        } else {
            Err(SterngateError::HalError(
                "OpenPort RX receiver unavailable".into(),
            ))
        }
    }

    async fn close(&mut self) -> Result<()> {
        if !self.is_open.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.is_open.store(false, Ordering::SeqCst);

        if let Some(OpenPortBackend::Hardware {
            handle,
            endpoint_out,
            interface_num,
            ..
        }) = self.backend.take()
        {
            let guard = handle.lock().await;
            let timeout = Duration::from_millis(500);

            // Close channel: atc5
            let close_ch = OpenPortCommand::CloseChannel {
                channel: CHANNEL_CAN,
            }
            .encode();
            let _ = guard.write_bulk(endpoint_out, &close_ch, timeout);

            // Reset interface: atz
            let reset_cmd = OpenPortCommand::Reset.encode();
            let _ = guard.write_bulk(endpoint_out, &reset_cmd, timeout);

            let _ = guard.release_interface(interface_num);
        }

        tracing::info!("OpenPort interface closed");
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_connected(&self) -> bool {
        self.is_open.load(Ordering::SeqCst)
    }

    async fn measure_battery_voltage(&mut self) -> Result<Option<f32>> {
        let is_hardware = matches!(self.backend, Some(OpenPortBackend::Hardware { .. }));
        if is_hardware {
            self.read_battery_voltage().await.map(Some)
        } else {
            // The simulated constant is a test fixture, not a measurement.
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_openport_simulated_lifecycle() {
        let (mut iface, _feed) = OpenPortInterface::new_simulated(12.65);
        assert!(!iface.is_connected());

        iface.open().await.unwrap();
        assert!(iface.is_connected());

        // Check voltage measurement
        let v = iface.read_battery_voltage().await.unwrap();
        assert!((v - 12.65).abs() < 0.001);
        assert_eq!(iface.last_voltage(), Some(12.65));

        // Check device info
        assert!(iface
            .device_info()
            .unwrap()
            .contains("Tactrix OpenPort 2.0"));

        iface.close().await.unwrap();
        assert!(!iface.is_connected());
    }

    #[tokio::test]
    async fn test_openport_simulated_loopback_frames() {
        let (mut iface, _feed) = OpenPortInterface::new_simulated(12.80);
        iface.open().await.unwrap();

        let tx_frame = CanFrame::new_standard(0x7E0, &[0x02, 0x10, 0x03]);
        iface.send(tx_frame.clone()).await.unwrap();

        let rx_frame = iface.recv().await.unwrap();
        assert_eq!(rx_frame.id, 0x7E0);
        assert_eq!(rx_frame.data, vec![0x02, 0x10, 0x03]);

        iface.close().await.unwrap();
    }
}
