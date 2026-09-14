//! Pure wire-protocol encoder and decoder for Tactrix OpenPort 2.0.
//!
//! Reverse-engineered from OpenPort 2.0 firmware and driver protocol:
//! - ASCII AT command layer (`ati`, `ata`, `atz`, `ato`, `atc`, `att`, `atf`, `atk`, `atr`)
//! - Raw binary CAN framing with microsecond hardware timestamps
//! - Direct ADC voltage telemetry on OBD-II Pin 16 (`VBAT`)
//! - Safe diagnostic operation with zero anti-clone wipe logic

use sterngate_core::CanFrame;

/// USB Vendor ID for Tactrix OpenPort (FTDI VID reused by Tactrix).
pub const TACTRIX_VENDOR_ID: u16 = 0x0403;

/// USB Product ID for standard OpenPort 2.0 J2534 interface.
pub const TACTRIX_PRODUCT_ID_OP20: u16 = 0xCC4D;

/// USB Product ID for composite OpenPort 2.0 interface.
pub const TACTRIX_PRODUCT_ID_COMPOSITE: u16 = 0xCC4C;

/// USB Product ID for OpenPort 2.0 bootloader mode.
pub const TACTRIX_PRODUCT_ID_BOOTLOADER: u16 = 0xCC4B;

/// Channel ID for ISO 9141 (K-Line).
pub const CHANNEL_ISO9141: u8 = 3;

/// Channel ID for ISO 14230 (KWP2000).
pub const CHANNEL_ISO14230: u8 = 4;

/// Channel ID for raw CAN (11-bit and 29-bit).
pub const CHANNEL_CAN: u8 = 5;

/// Channel ID for ISO 15765-2 over CAN.
pub const CHANNEL_ISO15765: u8 = 6;

/// PassThru flag for 29-bit extended CAN identifier.
pub const CAN_29BIT_ID_FLAG: u32 = 0x0000_0100;

/// Filter type: Pass Filter.
pub const FILTER_TYPE_PASS: u32 = 1;

/// Filter type: Block Filter.
pub const FILTER_TYPE_BLOCK: u32 = 2;

/// Filter type: Flow Control Filter (ISO 15765).
pub const FILTER_TYPE_FLOW_CONTROL: u32 = 3;

/// Packet type tags in OpenPort bulk IN responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    /// Normal received CAN frame.
    NormMsg = 0x00,
    /// Transmission completed indicator.
    TxDone = 0x10,
    /// Loopback received frame.
    TxLoopback = 0x20,
    /// Message end indication.
    MsgEnd = 0x40,
    /// Extended address message end indication.
    ExtAddrMsgEnd = 0x44,
    /// Loopback message end indication.
    LoopbackMsgEnd = 0x60,
    /// Normal message start indication.
    NormMsgStart = 0x80,
    /// Loopback message start indication.
    TxLoopbackStart = 0xA0,
}

impl PacketType {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x00 => Some(PacketType::NormMsg),
            0x10 => Some(PacketType::TxDone),
            0x20 => Some(PacketType::TxLoopback),
            0x40 => Some(PacketType::MsgEnd),
            0x44 => Some(PacketType::ExtAddrMsgEnd),
            0x60 => Some(PacketType::LoopbackMsgEnd),
            0x80 => Some(PacketType::NormMsgStart),
            0xA0 => Some(PacketType::TxLoopbackStart),
            _ => None,
        }
    }
}

/// Commands sent from host to OpenPort 2.0 via bulk OUT endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenPortCommand {
    /// Query device identification and firmware version (`ati`).
    Identify,
    /// Activate device transceiver (`ata`).
    Activate,
    /// Reset device channels to idle state (`atz`).
    Reset,
    /// Open a protocol channel (`ato`).
    OpenChannel { channel: u8, flags: u32, baud: u32 },
    /// Close an active protocol channel (`atc`).
    CloseChannel { channel: u8 },
    /// Transmit a raw CAN frame over the specified channel (`att`).
    TransmitCan { channel: u8, frame: CanFrame },
    /// Set a hardware message filter (`atf`).
    SetFilter {
        channel: u8,
        filter_type: u32,
        flags: u32,
        mask: Vec<u8>,
        pattern: Vec<u8>,
        flow_control: Option<Vec<u8>>,
    },
    /// Delete an active message filter (`atk`).
    DeleteFilter { channel: u8, filter_id: u32 },
    /// Read ADC voltage on the specified pin (`atr`). Pin 16 is OBD-II battery voltage.
    ReadPinVoltage { pin: u8 },
}

impl OpenPortCommand {
    /// Encode command to raw bytes for USB bulk OUT transfer.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            OpenPortCommand::Identify => b"\r\n\r\nati\r\n".to_vec(),
            OpenPortCommand::Activate => b"ata\r\n".to_vec(),
            OpenPortCommand::Reset => b"atz\r\n".to_vec(),
            OpenPortCommand::OpenChannel {
                channel,
                flags,
                baud,
            } => format!("ato{} {} {} 0\r\n", channel, flags, baud).into_bytes(),
            OpenPortCommand::CloseChannel { channel } => format!("atc{}\r\n", channel).into_bytes(),
            OpenPortCommand::TransmitCan { channel, frame } => {
                let flags = if frame.is_extended {
                    CAN_29BIT_ID_FLAG
                } else {
                    0
                };
                let payload_len = 4 + frame.data.len();
                let mut buf = format!("att{} {} {}\r\n", channel, payload_len, flags).into_bytes();
                // 4-byte Big-Endian CAN Arbitration ID
                buf.extend_from_slice(&frame.id.to_be_bytes());
                // Raw CAN frame payload bytes
                buf.extend_from_slice(&frame.data);
                buf
            }
            OpenPortCommand::SetFilter {
                channel,
                filter_type,
                flags,
                mask,
                pattern,
                flow_control,
            } => {
                let mask_len = mask.len();
                let mut buf = format!("atf{} {} {} {}\r\n", channel, filter_type, flags, mask_len)
                    .into_bytes();
                buf.extend_from_slice(mask);
                buf.extend_from_slice(pattern);
                if let Some(fc) = flow_control {
                    buf.extend_from_slice(fc);
                }
                buf
            }
            OpenPortCommand::DeleteFilter { channel, filter_id } => {
                format!("atk{} {}\r\n", channel, filter_id).into_bytes()
            }
            OpenPortCommand::ReadPinVoltage { pin } => format!("atr {}\r\n", pin).into_bytes(),
        }
    }
}

/// Decoded response packet from OpenPort 2.0 bulk IN endpoint.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenPortResponse {
    /// Firmware and hardware version info (`ari <version>`).
    DeviceInfo(String),
    /// Command execution success acknowledgement (`aro\r\n`).
    AckOk,
    /// Hardware filter successfully assigned with numeric ID (`arf <id>`).
    FilterAssigned(u32),
    /// Measured ADC voltage on pin in millivolts (`arr <pin> <mV>`).
    PinVoltage { pin: u8, millivolts: u32 },
    /// Received CAN frame with microsecond timestamp.
    ReceivedCan {
        channel: u8,
        frame: CanFrame,
        timestamp_us: u32,
    },
    /// Transmit complete acknowledgement.
    TxDone { channel: u8, timestamp_us: u32 },
    /// Device error response (`are <code...>`).
    DeviceError { code: u32, message: String },
}

/// Stream decoder for OpenPort bulk IN packets.
#[derive(Debug, Default)]
pub struct OpenPortDecoder {
    buffer: Vec<u8>,
}

impl OpenPortDecoder {
    /// Create a new empty decoder.
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Feed incoming raw USB bytes into the decoder buffer.
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Clear internal buffer.
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Return count of unparsed bytes currently in buffer.
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    /// Attempt to parse the next complete response from the buffer.
    /// Returns `None` if more bytes are required to form a full packet.
    pub fn next_response(&mut self) -> Option<OpenPortResponse> {
        loop {
            if self.buffer.is_empty() {
                return None;
            }

            // Look for header prefix 'ar'
            let mut prefix_pos = None;
            for i in 0..self.buffer.len().saturating_sub(1) {
                if self.buffer[i] == b'a' && self.buffer[i + 1] == b'r' {
                    prefix_pos = Some(i);
                    break;
                }
            }

            // If no 'ar' found, keep only the last byte if it's 'a', discard the rest
            let Some(start) = prefix_pos else {
                if self.buffer.last() == Some(&b'a') {
                    self.buffer = vec![b'a'];
                } else {
                    self.buffer.clear();
                }
                return None;
            };

            // Discard any garbage before 'ar'
            if start > 0 {
                self.buffer.drain(..start);
            }

            // We need at least 3 bytes: 'a', 'r', tag
            if self.buffer.len() < 3 {
                return None;
            }

            let tag = self.buffer[2];

            // 1. Check for ASCII text responses: 'o' (aro), 'i' (ari), 'f' (arf), 'r' (arr), 'e' (are)
            if matches!(tag, b'o' | b'i' | b'f' | b'r' | b'e') {
                // Look for \r\n terminating line
                let crlf_pos = self.buffer.windows(2).position(|w| w == b"\r\n")?;

                let line_bytes = self.buffer.drain(..crlf_pos + 2).collect::<Vec<u8>>();
                let text = String::from_utf8_lossy(&line_bytes[..crlf_pos]);
                let tokens: Vec<&str> = text.split_whitespace().collect();
                if tokens.is_empty() {
                    continue;
                }

                match tokens[0] {
                    "aro" => return Some(OpenPortResponse::AckOk),
                    "ari" => {
                        let version = tokens[1..].join(" ");
                        return Some(OpenPortResponse::DeviceInfo(version));
                    }
                    "arf" => {
                        if let Some(id_str) = tokens.get(1) {
                            if let Ok(id) = id_str.parse::<u32>() {
                                return Some(OpenPortResponse::FilterAssigned(id));
                            }
                        }
                        return Some(OpenPortResponse::AckOk);
                    }
                    "arr" => {
                        if tokens.len() >= 3 {
                            if let (Ok(pin), Ok(mv)) =
                                (tokens[1].parse::<u8>(), tokens[2].parse::<u32>())
                            {
                                return Some(OpenPortResponse::PinVoltage {
                                    pin,
                                    millivolts: mv,
                                });
                            }
                        }
                    }
                    "are" => {
                        let code = tokens
                            .get(1)
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(1);
                        let msg = tokens.get(2..).map(|s| s.join(" ")).unwrap_or_default();
                        return Some(OpenPortResponse::DeviceError { code, message: msg });
                    }
                    _ => continue,
                }
            }

            // 2. Check for binary CAN message packet: tag is channel ID ('5' or '6')
            if tag == CHANNEL_CAN || tag == CHANNEL_ISO15765 || tag == b'5' || tag == b'6' {
                // Header is 4 bytes: 'a', 'r', channel, payload_len
                if self.buffer.len() < 4 {
                    return None;
                }

                let payload_len = self.buffer[3] as usize;
                let total_packet_len = 4 + payload_len;

                if self.buffer.len() < total_packet_len {
                    // Packet incomplete, wait for more bytes
                    return None;
                }

                // Consume the full packet
                let packet = self.buffer.drain(..total_packet_len).collect::<Vec<u8>>();
                let channel_id = packet[2];

                if payload_len < 5 {
                    // Malformed payload (needs at least packet_type + 4-byte timestamp)
                    continue;
                }

                let packet_type = PacketType::from_u8(packet[4]);
                let timestamp_us = u32::from_be_bytes([packet[5], packet[6], packet[7], packet[8]]);

                match packet_type {
                    Some(PacketType::TxDone) => {
                        return Some(OpenPortResponse::TxDone {
                            channel: channel_id,
                            timestamp_us,
                        });
                    }
                    Some(PacketType::NormMsg)
                    | Some(PacketType::TxLoopback)
                    | Some(PacketType::MsgEnd) => {
                        // Needs at least 1 byte type + 4 bytes timestamp + 4 bytes CAN ID = 9 bytes
                        if payload_len >= 9 {
                            let can_id =
                                u32::from_be_bytes([packet[9], packet[10], packet[11], packet[12]]);
                            let data_slice = &packet[13..total_packet_len];

                            let is_extended = can_id > 0x7FF;
                            let frame = if is_extended {
                                CanFrame::new_extended(can_id, data_slice)
                                    .with_timestamp(timestamp_us as u64)
                            } else {
                                CanFrame::new_standard(can_id as u16, data_slice)
                                    .with_timestamp(timestamp_us as u64)
                            };

                            return Some(OpenPortResponse::ReceivedCan {
                                channel: channel_id,
                                frame,
                                timestamp_us,
                            });
                        }
                    }
                    _ => {
                        // Start indications or unhandled packets
                        continue;
                    }
                }
            }

            // If we got here and tag was unrecognized, discard 'ar' and continue
            self.buffer.drain(..2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_identify() {
        let cmd = OpenPortCommand::Identify;
        assert_eq!(cmd.encode(), b"\r\n\r\nati\r\n");
    }

    #[test]
    fn test_encode_activate_and_reset() {
        assert_eq!(OpenPortCommand::Activate.encode(), b"ata\r\n");
        assert_eq!(OpenPortCommand::Reset.encode(), b"atz\r\n");
    }

    #[test]
    fn test_encode_open_channel_can_500k() {
        let cmd = OpenPortCommand::OpenChannel {
            channel: CHANNEL_CAN,
            flags: 0,
            baud: 500000,
        };
        assert_eq!(cmd.encode(), b"ato5 0 500000 0\r\n");
    }

    #[test]
    fn test_encode_transmit_can_standard_frame() {
        let frame = CanFrame::new_standard(0x7E0, &[0x02, 0x10, 0x03]);
        let cmd = OpenPortCommand::TransmitCan {
            channel: CHANNEL_CAN,
            frame,
        };
        let encoded = cmd.encode();

        let header = b"att5 7 0\r\n";
        assert_eq!(&encoded[..header.len()], header);
        // ID 0x7E0 Big-Endian
        assert_eq!(
            &encoded[header.len()..header.len() + 4],
            &[0, 0, 0x07, 0xE0]
        );
        // Data payload
        assert_eq!(&encoded[header.len() + 4..], &[0x02, 0x10, 0x03]);
    }

    #[test]
    fn test_encode_read_battery_voltage_pin16() {
        let cmd = OpenPortCommand::ReadPinVoltage { pin: 16 };
        assert_eq!(cmd.encode(), b"atr 16\r\n");
    }

    #[test]
    fn test_decode_ascii_responses() {
        let mut decoder = OpenPortDecoder::new();

        // 1. Device Info
        decoder.feed(b"ari OpenPort 2.0 1.15.4123\r\n");
        let resp = decoder.next_response().expect("Expected response");
        assert_eq!(
            resp,
            OpenPortResponse::DeviceInfo("OpenPort 2.0 1.15.4123".to_string())
        );

        // 2. AckOk
        decoder.feed(b"aro\r\n");
        let resp = decoder.next_response().expect("Expected response");
        assert_eq!(resp, OpenPortResponse::AckOk);

        // 3. Filter Assigned
        decoder.feed(b"arf 42\r\n");
        let resp = decoder.next_response().expect("Expected response");
        assert_eq!(resp, OpenPortResponse::FilterAssigned(42));

        // 4. Pin Voltage (12.64V on pin 16)
        decoder.feed(b"arr 16 12640\r\n");
        let resp = decoder.next_response().expect("Expected response");
        assert_eq!(
            resp,
            OpenPortResponse::PinVoltage {
                pin: 16,
                millivolts: 12640
            }
        );
    }

    #[test]
    fn test_decode_binary_can_frame() {
        let mut decoder = OpenPortDecoder::new();

        // Construct raw binary packet for CAN RX:
        // 'a', 'r', '5', payload_len: 13 (1 type + 4 ts + 4 id + 4 data)
        let mut raw = vec![b'a', b'r', b'5', 13];
        raw.push(PacketType::NormMsg as u8); // 0x00
        raw.extend_from_slice(&1_250_000u32.to_be_bytes()); // timestamp 1.25s
        raw.extend_from_slice(&0x7E8u32.to_be_bytes()); // CAN ID 0x7E8 (EDC16)
        raw.extend_from_slice(&[0x03, 0x7F, 0x22, 0x11]); // Positive/Negative UDS response

        decoder.feed(&raw);
        let resp = decoder.next_response().expect("Expected response");

        match resp {
            OpenPortResponse::ReceivedCan {
                channel,
                frame,
                timestamp_us,
            } => {
                assert_eq!(channel, b'5');
                assert_eq!(frame.id, 0x7E8);
                assert!(!frame.is_extended);
                assert_eq!(frame.data, vec![0x03, 0x7F, 0x22, 0x11]);
                assert_eq!(timestamp_us, 1_250_000);
            }
            other => panic!("Unexpected response: {:?}", other),
        }
    }

    #[test]
    fn test_decoder_fragmented_stream() {
        let mut decoder = OpenPortDecoder::new();

        // Feed partial bytes
        decoder.feed(b"arr 16 ");
        assert_eq!(decoder.next_response(), None);

        decoder.feed(b"12480\r\n");
        let resp = decoder.next_response().expect("Expected response");
        assert_eq!(
            resp,
            OpenPortResponse::PinVoltage {
                pin: 16,
                millivolts: 12480
            }
        );
    }
}
