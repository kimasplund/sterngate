---
name: bench-recovery
description: >-
  Hardware pinouts, bench wiring harnesses, BDM probe frame positioning, and
  step-by-step unbricking / clone recovery procedures for Bosch EDC16C31 / CP31
  using K-Tag V7.020, BDM100, and Sterngate.
---

# Bosch EDC16 Bench Wiring, BDM Pinouts & ECU Recovery Runbook

> [!CAUTION]
> **High Risk Hardware Operation**: Opening an ECU and probing its BDM header bypasses all software safety interlocks.
> Incorrect voltage, reverse ribbon polarity, or misaligned pogo pins can instantly destroy the MPC5xx processor or parallel NOR Flash.
> Follow this runbook step-by-step.

---

## 1. Scope & Architecture

This runbook covers physical bench connection, hardware recovery, and cloning for Bosch EDC16 engine control units utilized across Mercedes-Benz CDI platforms:
- **EDC16C31**: OM646 (2.2L 4-cylinder), OM647 (2.7L 5-cylinder), OM648 (3.2L inline 6-cylinder)
- **EDC16CP31**: OM642 (3.0L V6 CDI)

### Component Layout
- **MCU**: Motorola/Freescale MPC555 / MPC556 / MPC562 / MPC564 (32-bit PowerPC architecture)
- **External Flash**: Spansion / AMD `AM29BL802CB` (1MB) or `S29CD016J` (2MB) parallel NOR Flash
- **Serial EEPROM**: STMicroelectronics `95160` (2KB), `95320` (4KB), or `95640` (8KB) containing vehicle VIN, FBS3 immobilizer hash keys, injector IMA calibrations, and operating hours.

```
       +-----------------------------------------------------------+
       |                  BOSCH EDC16 PCB TOP VIEW                 |
       |                                                           |
       |     +-------------------+          +-----------------+    |
       |     |  MPC5xx PowerPC   |          |  AM29BL802CB    |    |
       |     |   Microcontroller |          |  Flash Memory   |    |
       |     +-------------------+          +-----------------+    |
       |                                                           |
       |     [1 2]                                                 |
       |     [3 4]                                                 |
       |     [5 6]  <-- 14-Pin BDM Pad                             |
       |     [7 8]      Header                                     |
       |     ...                                                   |
       |                                     +--------------+      |
       |                                     | 95xxx EEPROM |      |
       |                                     +--------------+      |
       +-----------------------------------------------------------+
               |  Connector A (Large)  |  Connector K (Small)  |
               +-----------------------+-----------------------+
```

---

## 2. Opening the ECU Casing Safely

1. **Remove Perimeter Torx Screws**: 4x Torx T15/T20 screws holding the aluminum cover.
2. **Thermal Softening**: Bosch seals the casing with industrial polyacrylate/silicone. Use a heat gun set to $85^\circ\text{C}-95^\circ\text{C}$ around the perimeter for 2-3 minutes.
3. **Gentle Levering**:
   - Use a wide, flat plastic or dull pry tool starting at the connector corner.
   - **DO NOT** lever against internal SMD capacitors or inductors near the rim.
   - **DO NOT** bend the PCB. A bent multi-layer PCB breaks internal via traces, causing unrepairable damage.
4. **ESD Precaution**: Ground yourself with an anti-static wrist strap connected to bench chassis ground before touching the exposed board.

---

## 3. Motorola 14-Pin BDM Pad Header Pinout

Bosch EDC16 motherboards feature an unpopulated 14-pin dual-row footprint ($2 \times 7$, 1.27mm pitch pads) for background debug mode:

```
        Pin 1 (Red Stripe / Square Pad) [ ] [ ] Pin 2 (SRESET)
                 Pin 3 (Ground)         [ ] [ ] Pin 4 (HRESET)
                 Pin 5 (DSDI)           [ ] [ ] Pin 6 (FREEZE)
                 Pin 7 (DSCK)           [ ] [ ] Pin 8 (DSDO)
                 Pin 9 (VFLS0)          [ ] [ ] Pin 10 (VFLS1)
                 Pin 11 (VCC +3.3V)     [ ] [ ] Pin 12 (Ground)
                 Pin 13 (BKPT)          [ ] [ ] Pin 14 (VFLS2)
```

### Pin Description Table
| Pin | Signal | Direction | Description |
|:---:|:---|:---:|:---|
| **1** | `VCC / VDD3` | In/Out | +3.3V power sense (or probe reference power) |
| **2** | `SRESET` | Bidirectional | Soft System Reset (active low) |
| **3** | `GND` | Ground | Common system digital ground |
| **4** | `HRESET` | Bidirectional | Hard System Reset (halts CPU on boot) |
| **5** | `DSDI` | In | Development Serial Data In (commands into MPC5xx) |
| **6** | `FREEZE` | Out | CPU halted in debug mode indicator |
| **7** | `DSCK` | In | Development Serial Clock |
| **8** | `DSDO` | Out | Development Serial Data Out (telemetry & data stream) |
| **9** | `VFLS0` | Out | History buffer / instruction fetch tracking bit 0 |
| **10** | `VFLS1` | Out | History buffer / instruction fetch tracking bit 1 |
| **11** | `VCC_AUX` | Power | Secondary power rail |
| **12** | `GND` | Ground | Common system ground |
| **13** | `BKPT` | In | Breakpoint trigger (forces CPU into BDM state) |
| **14** | `VFLS2` | Out | Instruction fetch tracking bit 2 |

> [!IMPORTANT]
> The Pin 1 indicator is usually identified by a white dot, small arrow, or square copper solder pad on the motherboard. The red stripe of the BDM flat ribbon cable **must** align with Pin 1.

---

## 4. Bench Harness Connector Pinouts

To power the ECU on the test bench, connect a clean regulated $+12.0\text{ V}-13.8\text{ V}$ (minimum 3A) bench power supply to the main automotive harness connector pins:

### Bosch EDC16C31 / EDC16CP31 (Mercedes-Benz CDI)
| Harness Pin | Function | Bench Connection |
|:---|:---|:---|
| **Pin 1, 2, 4, 5** | Ground | DC Power Supply Negative (`-` GND) |
| **Pin 5, 6** (Small / Large) | Battery Constant (Terminal 30) | DC Power Supply Positive (`+12V` Constant) |
| **Pin 58** | Ignition Switch (Terminal 15) | DC Power Supply Positive (`+12V` Switched) |
| **Pin 72** | CAN-High | Tactrix / CAN-H (ISO 11898, 500 kbps) |
| **Pin 89** | CAN-Low | Tactrix / CAN-L (ISO 11898, 500 kbps) |
| **Pin 25** | K-Line (ISO 9141) | Optional diagnostic line for legacy KWP2000 |

```
                       BENCH POWER SUPPLY (13.5V, 5A)
                       +----------------------------+
                       | [ +12V RED ]  [ - GND BLK ]|
                       +------+---------------+-----+
                              |               |
               +--------------+               |
               |                              |
      +--------+--------+                     |
      | Switch (Term 15)|                     |
      +--------+--------+                     |
               |                              |
               v                              v
      [Pin 58 Ignition]               [Pins 1, 2, 4 GND]
      [Pins 5, 6 Batt +]
               |                              |
               +--------------+---------------+
                              |
                     +--------v--------+
                     | Bosch EDC16 ECU |
                     |   (Connector)   |
                     +--------+--------+
                              |
               [Pin 72 CAN-H] | [Pin 89 CAN-L]
                              v
                     +-----------------+
                     | Tactrix OpenPort| --> USB to Sterngate Host
                     | / SocketCAN Int |
                     +-----------------+
```

---

## 5. BDM Positioning Frame & Probe Setup

1. **Mount Frame**: Clamp the acrylic or metal BDM positioning frame firmly to the workbench.
2. **Select Bosch Adapter**: Insert the 14-pin Bosch-type spring-loaded pogo pin adapter into the vertical guide rail.
3. **Connect Ribbon Cable**:
   - Connect the flat grey ribbon cable between the K-Tag / BDM100 unit and the adapter probe.
   - Verify Pin 1 orientation: Red stripe on ribbon connects to Pin 1 on both programmer and adapter.
4. **Lower Probe Slowly**:
   - Turn the vertical screw wheels to lower the pogo pins until they touch the 14 copper pads on the PCB.
   - Observe pogo pin compression: compress approximately 1-2 mm to ensure positive mechanical contact.
   - Turn on probe adapter LED lighting to verify exact alignment. Every pin must center on its pad.
5. **Continuity Check**:
   - Using a multimeter in continuity mode, check resistance between Pin 3/12 of the probe and the ECU metal casing. It must measure $< 0.5\ \Omega$.

---

## 6. Recovery & Unbricking Procedures

### Scenario A: Soft-Bricked ECU (CAN Bootloader Accessible)
The ECU was interrupted during OBD flashing. The application firmware is corrupt, but the bootloader still responds on CAN (`0x7E0`):

> [!NOTE]
> A recovery image sourced from an SDflash `.cff`/`.smr-f` container is never stageable directly — the vault and `/api/v1/flash/stage` refuse it as a Caesar flash container. Run it through `sterngate corpus extract` (Phase 1) first to pull the raw ROM segment into the vault as a stageable image. A BDM bench read of an EDC16CP31's *internal* MCU flash captures block D only (the bootloader/MCU code at `0x400000`); it carries no calibration data, so it cannot substitute for the external flash dump when you need the tune.

1. Connect the bench harness to the ECU (Power, Ground, CAN-H, CAN-L).
2. Connect Tactrix OpenPort 2.0 to your laptop running Sterngate.
3. Verify connection with Sterngate CLI:
   ```bash
   sterngate diag dtc --module EDC16 --interface openport
   ```
4. Find the matching stock firmware binary in your local vault:
   ```bash
   sterngate flash vault-scan --hw-id 0281013
   ```
5. Initiate safe recovery flash with forced power supply override:
   ```bash
   sterngate flash start --manifest firmware_vault/edc16_w211_manifest.json --rom firmware_vault/stock_flash.bin --force-yes
   ```

---

### Scenario B: Hard-Bricked ECU (Completely Dead, No CAN Response)
The erase routine (`0x31 FF00`) wiped the flash or corrupt code caused the MCU to enter an infinite reset loop:

1. **Hardware Setup**:
   - Position ECU under BDM frame with pogo pins seated on the 14 pads.
   - Connect external 12V DC power to the bench connector (Pins 1/2 Ground, Pins 5/6 Batt, Pin 58 Ign).
   - Plug K-Tag / BDM100 USB into the computer.
2. **K-Tag Protocol Selection**:
   - Open KSuite software.
   - Select: `Car` $\rightarrow$ `Mercedes` $\rightarrow$ `E-Class (W211)` $\rightarrow$ `E280/E320 CDI Bosch EDC16+ CP31`.
   - Protocol: **Protocol 060** (or Protocol 061 for EDC16C31).
3. **Read & Full Backup (Crucial First Step)**:
   - Click **Backup**.
   - K-Tag will halt the MPC5xx CPU via `BKPT` and dump:
     * `micro.mpc` (Internal MCU code, 512KB)
     * `flash.bin` (External AM29BL802CB flash, 1024KB or 2048KB)
     * `eeprom.bin` (ST95xxx serial EEPROM, 4KB or 8KB)
   - Store these original dump files in a timestamped backup directory.
4. **Restore / Unbrick**:
   - Load clean, verified flash binary from your Sterngate vault (`flash.bin`).
   - In K-Tag, choose **Write Flash** (leave EEPROM untouched!).
   - Wait for write and verification progress (approx. 60-90 seconds).
   - K-Tag will calculate and apply MPC/Bosch checksums automatically.
5. **Re-Test on CAN**:
   - Raise the BDM probe frame.
   - Cycle 12V ignition power (Pin 58).
   - Run Sterngate to confirm vehicle response:
     ```bash
     sterngate diag dtc --module EDC16 --interface openport
     ```

---

### Scenario C: ECU Cloning (Replacing a Damaged ECU)
Replacing a burned or water-damaged ECU with a junkyard donor unit:

1. Read **Full Backup** from original ECU via BDM (all 3 files: `.mpc`, `.bin`, `.eep`).
2. If original ECU EEPROM is readable, read **Full Backup** from donor ECU and save as baseline.
3. Write original `.eep` (EEPROM) to donor ECU. This transfers:
   - VIN
   - FBS3 Drive Authorization Hash
   - Injector IMA Codes
   - Odometer / Service History
4. Write target software flash (`.bin`) matching the vehicle equipment.
5. Test start vehicle: no dealer DAS/Xentry coding or virginization needed.

---

## 7. Emergency Checklist & Troubleshooting

| Symptom | Probable Cause | Corrective Action |
|:---|:---|:---|
| **"Communication error with ECU" in K-Tag** | 12V bench power disconnected or below 11.8V | Check power supply output; verify switched ignition (Pin 58) has +12V. |
| **"Hardware configuration not supported"** | BDM pogo pins misaligned or dirty pads | Clean pads with 99% isopropyl alcohol and fiberglass brush. Re-center probe. |
| **Pin 1 reversed** | Ribbon cable plugged upside down | Disconnect power immediately. Verify red stripe aligns with square pad #1. |
| **"Start Error" on vehicle cluster after flashing** | Corrupted EEPROM or mismatched IMMO hash | Re-flash original backup `eeprom.bin` via BDM. Never write random EEPROM files. |
| **ECU draws $> 1.5\text{ A}$ idle current on bench** | Short circuit across BDM pins or internal diode failure | Shut off power supply immediately; check with thermal camera for overheated components. |

