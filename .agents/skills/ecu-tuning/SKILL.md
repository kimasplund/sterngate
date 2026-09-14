---
name: ecu-tuning
description: >-
  Bosch EDC16/EDC17 calibration map scanning, Stage 1 (+18%) and Stage 2 (+25%) generation,
  DTC suppression tables, Bosch MPC5xx partitioned 32-bit block checksum recalculation, and
  safe .sgmod deployment in Sterngate.
---

# Bosch EDC16/EDC17 ECU Tuning, Map Calibration & DTC Suppression Guide

> [!CAUTION]
> **ECU Bricking Prevention**: Automotive ECUs will refuse to boot or enter an unrecoverable bootloader state if flash checksums are invalid.
> Always verify Bosch MPC5xx 32-bit block checksums before flashing or writing calibration tables.
> Never deploy calibration changes when vehicle battery voltage is $< 12.5\text{ V}$.

---

## 1. Overview & Supported ECUs

Sterngate includes a built-in reverse-engineered calibration engine capable of directly parsing, modifying, and recalculating checksums on Bosch engine control unit binary dumps:
- **Bosch EDC16C31**: Mercedes OM646 (2.2L), OM647 (2.7L), OM648 (3.2L CDI)
- **Bosch EDC16CP31**: Mercedes OM642 (3.0L V6 CDI)
- **Bosch EDC17**: VAG TDI, BMW N47/N57, Mercedes OM651 (TriCore TC17xx architecture)

---

## 2. Bosch Map Detection Engine

Sterngate's `BoschMapDetector` scans raw binary ROM dumps to locate characteristic 2D and 3D calibration maps using Bosch header identifiers and axis dimension headers:

| Map Type | Typical Dimensions | Description & Units | Safe Scaling Factor |
| :--- | :--- | :--- | :--- |
| **Driver Wish** | $12 \times 16$ or $16 \times 16$ | Pedal position ($\%$) vs. RPM $\to$ Target torque ($\text{Nm}$) | $+15\%\text{ to }+20\%$ at $>60\%$ pedal |
| **Torque Limiter** | $1 \times 16$ or $1 \times 20$ | Atmospheric pressure / RPM $\to$ Maximum engine torque ($\text{Nm}$) | $+18\%\text{ (Stage 1) / }+25\%\text{ (Stage 2)}$ |
| **Smoke Limiter** | $16 \times 16$ | Air mass ($\text{mg/hub}$) vs. RPM $\to$ Maximum injected fuel quantity | Modulated to maintain stoichiometric $\lambda \ge 1.15$ |
| **Boost Target (MAP)** | $16 \times 16$ | Injected quantity vs. RPM $\to$ Manifold Absolute Pressure ($\text{mbar}$) | $+120\text{ mbar (Stage 1) / }+200\text{ mbar (Stage 2)}$ |
| **SVBL (Single Value Boost Limit)** | 16-bit scalar | Absolute hardware turbo protection cutoff limit | Set $+150\text{ mbar}$ above peak boost target |
| **Common Rail Pressure** | $16 \times 16$ | Injected quantity vs. RPM $\to$ Target fuel rail pressure ($\text{bar}$) | $+50\text{ bar (Stage 1) / }+80\text{ bar (Stage 2)}$ |
| **EGR Hysteresis** | $2 \times 16$ or $2 \times 8$ | Temperature vs. RPM enable/disable switches for Exhaust Gas Recirculation | Zeroed out for EGR Delete |

---

## 3. Automated Stage Calibration Profiles

### Stage 1 (Safe OEM Hardware Tolerance)
- **Peak Torque**: $+18\%$ over stock curve.
- **Boost Pressure**: $+120\text{ mbar}$ target increase.
- **Rail Pressure**: $+50\text{ bar}$ increase at high load ($>1800\text{ bar}$ maximum).
- **DPF / EGR**: Retains 100% factory emissions compliance, soot regeneration, and readiness codes.
- **Hardware Prerequisites**: 100% factory stock vehicle in good mechanical order.

### Stage 2 (Performance Downpipe / Delete)
- **Peak Torque**: $+25\%$ over stock curve.
- **Boost Pressure**: $+200\text{ mbar}$ target increase.
- **Rail Pressure**: $+80\text{ bar}$ increase at high load.
- **DPF Delete**: Disables DPF regeneration triggers, soot calculation models, and differential pressure sensors.
- **EGR Hysteresis Zeroing**: Closes EGR valve permanently to stop intake soot build-up.
- **DTC Error Suppression**: Automatically suppresses DTCs `P0401` (EGR Insufficient Flow), `P0402` (EGR Excessive Flow), `P2002` (DPF Efficiency Below Threshold), `P2463` (DPF Soot Accumulation).
- **Hardware Prerequisites**: Aftermarket high-flow downpipe or DPF delete pipe installed.

---

## 4. DTC Suppression (Error Switch Table Zeroing)

Instead of brutally disabling entire diagnostic subsystems or clearing fault codes every key cycle, Sterngate uses precision **DTC Error Switch Zeroing**:
1. Locates the Bosch DTC master table in Flash ROM.
2. Identifies the specific 8-bit or 16-bit enable switch corresponding to the SAE P-code (e.g., `P0401`, `P2002`).
3. Overwrites only the single-byte enable switch with `0x00`, leaving the remaining diagnostic system fully operational.

---

## 5. Bosch MPC5xx 32-Bit Partitioned Block Checksums

Motorola/Freescale MPC5xx series processors calculate checksums across multiple discrete flash blocks:
1. **Bootloader Block**: Protects initial boot code and vector tables ($0\text{x}000000\text{--}0\text{x}01FFFF$).
2. **Firmware Core**: Main operating logic and UDS protocol stack ($0\text{x}020000\text{--}0\text{x}07FFFF$).
3. **Calibration / Maps Block**: All 2D/3D performance maps ($0\text{x}080000\text{--}0\text{x}0FFFFF$).

The algorithm sums 32-bit big-endian words across each block. If the sum does not match the stored vector at the end of the partition, the ECU will reject ignition or trigger limp mode.
Sterngate's `BoschChecksumSolver` verifies and recalculates all blocks in milliseconds:

```bash
sterngate tune checksum --rom /path/to/modified.bin --fix --output /path/to/recalculated.bin
```

---

## 6. Operational Workflows

### CLI Workflow
```bash
# Step 1: Scan stock ROM
sterngate tune scan --rom stock_om646.bin

# Step 2: Generate Stage 1 Mod Package
sterngate tune stage1 --rom stock_om646.bin \
  --chassis "W211 E220 CDI" --ecu EDC16C31 --output stage1_om646.sgmod --armor

# Step 3: Inspect compatibility
sterngate mod inspect --input stage1_om646.sgmod

# Step 4: Apply to vehicle
sterngate mod apply --input stage1_om646.sgmod --vin WDB2112061A000001
```

### Model Context Protocol (MCP) Workflow
1. Call `sterngate_scan_rom_maps` with `{"rom_path": "/path/to/rom.bin"}`.
2. Call `sterngate_generate_stage_tune` with `{"rom_path": "/path/to/rom.bin", "stage": 1, "chassis": "W211", "ecu_name": "EDC16"}`.
3. Review the returned `.sgmod` package and summary metrics.
4. Call `sterngate_apply_community_mod` with the generated package.
