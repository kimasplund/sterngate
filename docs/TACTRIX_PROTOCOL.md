# Tactrix OpenPort 2.0 Reverse-Engineered Wire Protocol & Linux Driver Handbook

This document provides the definitive specification of the Tactrix OpenPort 2.0 USB wire protocol, decoded via reverse-engineering of official installer packages, Windows KMDF drivers (`openport.sys`), J2534 libraries (`op20pt32.dll`), and USB bulk stream captures.

---

## 1. Executive Summary & Why Reverse Engineering was Necessary

Tactrix originally released the OpenPort 2.0 around 2008 as an affordable SAE J2534 PassThru cable. However:
1. **Vendor Abandonment**: Tactrix halted OpenPort 2.0 software updates around 2016 to develop OpenPort 3.0, leaving OpenPort 2.0 locked to 32-bit Windows drivers and unmaintained closed DLLs.
2. **Anti-Clone Bricking**: Official Windows software (EcuFlash and `op20pt32.dll`) actively checks device serial numbers online against Tactrix databases. When an unapproved or clone serial is detected, the software issues LPC2368 flash erase commands, irreversibly bricking the device.
3. **Linux Exclusion**: The vendor never released a native Linux kernel or userspace driver. Users were forced to use complex Wine bridges, VM passthrough, or outdated Windows XP/7 laptops.

**Sterngate's clean-room driver (`OpenPortInterface`) solves all three problems**:
- Communicates directly over USB bulk endpoints via `rusb` on Linux with zero Windows code.
- Strips out all phone-home telemetry and erase routines, guaranteeing **100% clone safety**.
- Interrogates the built-in OBD-II Pin 16 ADC to enforce safe ECU flashing voltage gates ($\ge 12.5\text{ V}$) without external hardware multimeters.

---

## 2. Hardware Architecture & USB Identifiers

The OpenPort 2.0 is powered by an **NXP LPC2368** microcontroller (ARM7TDMI-S with 512KB Flash, 32KB RAM, dual CAN controllers, full-speed USB 2.0 device controller):
* **Vendor ID (VID)**: `0x0403` (FTDI Vendor ID repurposed by Tactrix)
* **Product ID (PID)**:
  - `0xCC4D`: Standard J2534 interface
  - `0xCC4C`: Composite device interface (`MI_00`)
  - `0xCC4B`: LPC2368 DFU / Bootloader mode

### USB Endpoints
- **Bulk OUT**: Typically Endpoint 1 or 2 (`0x01` or `0x02`), used for host-to-device command dispatch.
- **Bulk IN**: Typically Endpoint 1 or 2 (`0x81` or `0x82`), used for device-to-host frame and telemetry streaming.

---

## 3. Protocol Wire Syntax

Commands sent to Bulk OUT follow an ASCII AT prefix followed by arguments, terminating with `\r\n`. Binary payloads (e.g. CAN arbitration IDs, payload data, filter masks) are appended immediately after `\r\n`.

### Command Reference

| Command | Wire Format | Arguments & Semantics | Response |
| :--- | :--- | :--- | :--- |
| **Identify** | `\r\n\r\nati\r\n` | Queries hardware model and firmware version string | `ari <version>\r\n` |
| **Activate** | `ata\r\n` | Powers transceiver and activates CAN controller | `aro\r\n` |
| **Reset** | `atz\r\n` | Closes open channels and returns device to idle | `aro\r\n` |
| **Open Channel** | `ato<chan> <flags> <baud> 0\r\n` | `<chan>`: `5`=CAN, `6`=ISO15765.<br>`<flags>`: `0`=11-bit, `0x100`=29-bit.<br>`<baud>`: e.g. `500000`. | `aro\r\n` |
| **Close Channel** | `atc<chan>\r\n` | Closes the specified protocol channel | `aro\r\n` |
| **Transmit Frame** | `att<chan> <len> <flags>\r\n<data>` | `<len>`: $4 + \text{payload length}$.<br>`<data>`: 4 bytes big-endian CAN ID + payload. | None or `aro\r\n` |
| **Set Filter** | `atf<chan> <type> <flags> <len>\r\n<mask...><pattern...>` | Sets hardware filter ID.<br>`<type>`: 1=PASS, 2=BLOCK, 3=FLOW_CONTROL. | `arf <filter_id>\r\n` |
| **Delete Filter** | `atk<chan> <filter_id>\r\n` | Deletes existing message filter | `aro\r\n` |
| **Read Pin Voltage**| `atr <pin>\r\n` | Reads ADC millivolts on pin (`16` = OBD-II VBAT) | `arr 16 <millivolts>\r\n` |

---

## 4. Incoming Stream Parsing (Bulk IN)

The device sends packets tagged with `ar`:

```
+----+----+---------+-----+------+---------+--------+---------+
| 'a'| 'r'| Channel | Len | Type | Ts (4B) | ID (4B)| Payload |
+----+----+---------+-----+------+---------+--------+---------+
  0    1       2       3      4     5..8     9..12    13..end
```

* **Byte 2 (Channel / Tag)**:
  - `'o'` (`0x6F`): Acknowledgement (`aro\r\n`)
  - `'i'` (`0x69`): Info string (`ari ...\r\n`)
  - `'f'` (`0x66`): Filter ID (`arf <id>\r\n`)
  - `'r'` (`0x72`): Voltage reading (`arr <pin> <mV>\r\n`)
  - `'e'` (`0x65`): Error code (`are <code...>\r\n`)
  - `'5'` (`0x35`) / `'6'` (`0x36`): CAN binary frame packet!
* **Byte 3 (`Len`)**: Payload length ($5 + \text{data bytes}$).
* **Byte 4 (`Type`)**:
  - `0x00`: Normal received message (`NORM_MSG`)
  - `0x10`: Transmit complete indicator (`TX_DONE`)
  - `0x20`: Loopback frame (`TX_LB_MSG`)
  - `0x40`: Message end indicator (`RX_MSG_END_IND`)
  - `0x80`: Message start indicator (`NORM_MSG_START_IND`)
* **Bytes 5..8 (`Ts`)**: 32-bit hardware timestamp in microseconds (Big-Endian).
* **Bytes 9..12 (`ID`)**: 32-bit CAN arbitration identifier (Big-Endian).
* **Bytes 13..**: Raw payload bytes (0 to 8 bytes).

---

## 5. Hardware Battery Voltage Interlock (Pin 16 ADC)

Automotive ECU flashing requires a minimum system voltage (typically $\ge 12.5\text{ V}$) to prevent flash memory write failures if voltage sags.

OpenPort 2.0 has an internal voltage divider and 10-bit analog-to-digital converter connected to J1962 / OBD-II Pin 16:
```
Host -> OpenPort: atr 16\r\n
OpenPort -> Host: arr 16 12640\r\n
```
Sterngate parses `12640` as $12.64\text{ V}$. If the measured voltage is $< 12.5\text{ V}$, Sterngate's safety gate automatically aborts erase and flashing routines (`SterngateError::VoltageTooLow { current: 12.48, required: 12.50 }`).

---

## 6. Linux Setup & Permissions

To use OpenPort 2.0 on Linux without root/sudo privileges:

```bash
# 1. Copy udev rules
sudo cp scripts/99-tactrix-openport.rules /etc/udev/rules.d/

# 2. Reload udev
sudo udevadm control --reload-rules
sudo udevadm trigger

# 3. Ensure user is in plugdev group
sudo usermod -aG plugdev $USER
```

