# Sterngate (`sterngate`)

[![Rust](https://img.shields.io/badge/rust-1.89%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![MCP](https://img.shields.io/badge/MCP-2024--11--05-brightgreen.svg)](https://modelcontextprotocol.io)

**Sterngate** is a high-performance, modular automotive diagnostic, live telemetry, variant coding, and safe ECU flashing platform written in modern Rust. It replaces legacy, proprietary OEM diagnostic tooling (such as Mercedes Vediamo and Star / Xentry) with an open-source, cross-platform architecture that eliminates Windows XP virtual machines, license dongles, and fragile COM port setups.

> [!CAUTION]
> ### LEGAL DISCLAIMER & ASSUMPTION OF RISK
> **STERNGATE COMMUNICATES DIRECTLY WITH SAFETY-CRITICAL AUTOMOTIVE CONTROLLERS (ENGINE ECUs, TRANSMISSIONS, SENSOTRONIC BRAKE CONTROL / SBC, SUSPENSION HYDRAULICS, AND VEHICLE GATEWAYS).**
>
> **YOU USE THIS SOFTWARE ENTIRELY AT YOUR OWN RISK.** The authors, copyright holders, and contributors accept **ABSOLUTELY NO LIABILITY** for damaged, corrupted, or bricked ECUs, immobilized vehicles, mechanical failures, personal injury, traffic accidents, towing fees, dealer recovery expenses, or financial damages arising from the use or misuse of this software.
>
> Modifying vehicle firmware, variant coding, or triggering high-pressure workshop service routines carries inherent risks. Always use a commercial voltage-regulated power supply ($\ge 12.5\text{ V}$, $\ge 25\text{ A}$). By downloading, compiling, or executing this software, you agree to all terms in [DISCLAIMER.md](DISCLAIMER.md).

---

## Key Capabilities

- **13 Mercedes Cascades of Death Monitored**: Autonomous early warning watchdog that detects failing $1.50–$160 wear parts before they cause catastrophic $2,000–$10,000+ mechanical or electrical destruction (SBC accumulator, Black Death washers, pilot bushing ATF wicking, TCC slip, DPF/M55 swirl motor, camshaft magnets, air suspension compressor burnout, ABC pulsation damper surge, ESL steering lock seizure, M272/M273 balance shaft wear, Valeo radiator glycol contamination, SAM water ingress, and OM642 oil cooler starvation).
- **Automated ECU Calibration & Stage 1/2 Tuning Engine**: Reverse-engineered Bosch map detector scans raw ROM binaries to locate Driver Wish, Torque Limiter, Smoke Limiter ($\lambda \ge 1.15$), Boost Target, Rail Pressure, and SVBL scalars. Automatically synthesizes verified Stage 1 (+18% torque, +120 mbar boost, +50 bar rail) and Stage 2 (+25% torque, +200 mbar boost, DPF delete, EGR zeroing) calibration packages.
- **Bosch MPC5xx 32-Bit Partitioned Block Checksums**: Autonomous checksum engine recalculates 32-bit inverted carry checksums across Bootloader, Firmware Core, and Calibration blocks in milliseconds, eliminating ECU bricking after tuning.
- **Precision DTC Suppression (Error Switch Table Zeroing)**: Selectively zeroes individual 8-bit/16-bit error enable switches in the Flash ROM table (e.g. `P0401`, `P2002`) without disabling unrelated diagnostic trouble codes.
- **Shareable Community Mod Packages (`.sgmod`)**: Secure, portable calibration format featuring ASCII armor and Reed-Solomon $GF(2^8)$ error-correction parity—capable of auto-repairing up to 8 damaged bytes from copy-paste corruption.
- **Donor Replacement ECU Re-VIN Adaptation**: Seamlessly pairs salvage/donor engine control modules by unlocking seed-key SecurityAccess, reprogramming the 17-character vehicle identification number via UDS DID 0xF190, and snapshotting the change in Git garage history.
- **One-Click Guided Workshop Procedures**: Pre-tested workshop routines for VMax top speed limiter configuration, AdBlue/SCR emergency lockout reset, EGR soot reduction (+40mg offset), seatbelt chime muting, exact tank liters display (Restliteranzeige), and cornering fog lights.
- **Active Hardware Safety Guards**: Software-controlled kill-switches and pressure limiters:
  * **ENR / AIRMATIC Compressor Guard**: Autonomous thermal watchdog with 40s continuous run cutoff, 180s cooldown, and Routine `0x0210` safe mode relay disconnect.
  * **ABC Hydraulic Surge Limiter**: Routine `0x0220` pressure dump (reduce 200 bar to 120 bar safe fallback) and Routine `0x0221` strut isolation valve lock to prevent line explosion over hot exhaust.
- **Native Linux Tactrix OpenPort 2.0 Driver**: Reverse-engineered direct USB bulk protocol via `rusb` (`--openport`). Eliminates abandoned 32-bit Windows drivers and Wine. **100% Clone Safe**—strips out vendor phone-home and anti-clone flash erase commands that brick Chinese clones on Windows. Includes direct OBD-II Pin 16 ADC hardware voltage monitoring to enforce the $\ge 12.5\text{ V}$ flashing interlock.
- **Vehicle Garage & Per-Car Git Configuration Tracking**: Automatically identifies and decodes VINs (e.g. S211 Estate OM646), creates an isolated Git repository under `data/vehicles/<VIN>/`, and commits every diagnostic scan, live vital snapshot, and variant coding session with full history and rollback capability.
- **In-Flight Drive Benchmarking & A/B Comparative Analysis**: High-frequency drive telemetry sampling (consumption, boost, rail pressure, coolant, TCC lockup slip) with mathematical diesel consumption modeling and A/B comparison to verify if tuning, adaptations, or hardware changes were beneficial.
- **1,340+ Canonical ECU Catalog & 17,800+ DTC Dictionary**: Comprehensive routing index mapping CAN Tx/Rx IDs, UDS/KWP2000 protocols, functional IDs, and multi-chassis coverage for 1,347 automotive electronic control units across classic and modern platforms (W203 through W223), paired with a dictionary of 17,857 Mercedes diagnostic trouble codes in German and English.
- **Multilingual Diagnostics**: Fully localized in English (`en`), German (`de`, authentic Daimler OEM terms), and Swedish (`sv`) across all DTCs, routines, parameters, CLI output, and web dashboard.
- **Beyond Generic OBD-II**: Interrogates manufacturer-specific DIDs through the vehicle's Central Gateway (CGW) using UDS (ISO 14229) and KWP2000 (ISO 14230). Read 722.6 automatic transmission fluid temperatures (for the crucial 80°C level check), cylinder-by-cylinder smooth running injector balances, torque converter clutch slip, and Airmatic line pressures.
- **Universal Modularity**: Decouples vehicle profiles from executable code. Profiles are stored in declarative JSON schemas under `profiles/`. Switch between a Mercedes W211 OM646 CDI, a VAG Golf Mk6 2.0 TDI (EDC17 + DSG), or a BMW E90 3.0d (DDE6) without recompiling the binary.
- **Decoupled Safe Flashing**: Atomic local file staging, strict pre-flight safety gates (voltage $\ge 12.5\text{ V}$, SHA256 & Bosch CRC32 verification, HW/SW calibration match), and a detached Tokio worker immune to browser closes or network drops.
- **P2P Remote Operations (Iroh)**: Integrated peer-to-peer QUIC tunneling allows an end customer to plug the device into their OBD port and share a short Node Ticket with a remote technician anywhere in the world—punching through carrier-grade NATs without port forwarding.
- **Built-in Model Context Protocol (MCP) Server**: 38 standardized tools across 7 domains exposing live telemetry, diagnostic scanning, 13 cascades of death, flashing safety gates, ECU tuning, DTC suppression, and checksum recalculation directly to AI agents.
- **Hardware Abstraction Layer (HAL)**: Native support for Linux SocketCAN (`can0`, CANable, gs_usb, Candlelight, SPI MCP2518FD), native Tactrix OpenPort 2.0 (`--openport`), SAE J2534 PassThru, and zero-hardware virtual simulation.

---

## Architecture Overview

```
                        ┌───────────────────────────────────────────────┐
                        │        Browser Dashboard / Remote Tech UI     │
                        │    (Live Gauges, DTC Scanner, Variant Coding) │
                        └───────────────────────▲───────────────────────┘
                                                │ WebSocket / HTTP (Port 8080)
┌───────────────────────────────────────────────▼───────────────────────────────────────────────┐
│ Sterngate Core Daemon (Rust Workspace)                                                        │
│                                                                                               │
│  ┌─────────────────────────┐  ┌──────────────────────────┐  ┌──────────────────────────────┐  │
│  │ sterngate-server (Axum) │  │ sterngate-mcp (JSON-RPC) │  │ sterngate-p2p (Iroh Mesh)    │  │
│  └────────────┬────────────┘  └────────────┬─────────────┘  └──────────────┬───────────────┘  │
│               │                            │                               │                  │
│  ┌────────────▼────────────────────────────▼───────────────────────────────▼───────────────┐  │
│  │ sterngate-protocol (ISO-TP ISO 15765-2 / UDS ISO 14229 / KWP2000 / Seed-Key Solvers)    │  │
│  └─────────────────────────────────────────┬───────────────────────────────────────────────┘  │
│                                            │                                                  │
│  ┌─────────────────────────────────────────▼───────────────────────────────────────────────┐  │
│  │ sterngate-hal (Hardware Abstraction Layer)                                              │  │
│  │  • SocketCanInterface (can0)   • VirtualCanInterface (Mock)   • J2534 (Tactrix)        │  │
│  └─────────────────────────────────────────┬───────────────────────────────────────────────┘  │
└────────────────────────────────────────────┼──────────────────────────────────────────────────┘
                                             │ CAN High (Pin 6) & CAN Low (Pin 14)
                        ┌────────────────────▼────────────────────┐
                        │         Vehicle Central Gateway         │
                        │    (Routes to Engine, Gearbox, ENR)     │
                        └─────────────────────────────────────────┘
```

---

## Getting Started

### 1. Requirements
- Rust 1.89+ (2021/2024 edition compatible)
- Linux (for SocketCAN) or any platform (for Mock simulation and J2534)

### 2. Build the Workspace
```bash
cargo build --release
```

### 3. Run in Offline Mock Mode (Zero Hardware Needed)
```bash
cargo run --package sterngate-cli -- mock --port 8080
```
Open [http://localhost:8080](http://localhost:8080) in your web browser to explore the dashboard with simulated OM646 engine and 722.6 transmission telemetry.

### 4. Operational Modes

#### A. Local Standalone Mode (In-Car Raspberry Pi / Laptop / SBC)
```bash
# Using native Linux SocketCAN (e.g. CANable, gs_usb, SPI MCP2518FD)
sterngate --local --can-interface can0 --port 8080

# Using native Linux Tactrix OpenPort 2.0 (USB bulk interface)
sterngate --local --openport --port 8080

# The dashboard binds to 127.0.0.1 by default. The diagnostic API is
# unauthenticated and can actuate the vehicle, so expose it deliberately:
sterngate --local --can-interface can0 --bind 0.0.0.0 --port 8080

# Firmware vault location. Vault scans and staging are confined to this
# directory. Also settable via STERNGATE_VAULT_ROOT; defaults to ./firmware_vault
sterngate --local --vault /srv/sterngate/firmware --port 8080
```

#### B. Car-Side Diagnostic Bridge & P2P Host (`--bridge` / `--car`)
```bash
# Bridging SocketCAN (generates P2P ticket and listens)
sterngate --bridge --can-interface can0

# Bridging native Linux Tactrix OpenPort 2.0
sterngate --bridge --openport
```
Prints an encrypted Iroh Node Ticket to share with the remote technician.

#### C. Remote Technician Client Mode (`--tech` / `--ticket`)
```bash
# Dial the car-side bridge and open local technician dashboard on port 3000
sterngate --tech --ticket <NODE_TICKET> --port 3000

# Shortcut (passing --ticket automatically activates technician client mode)
sterngate --ticket <NODE_TICKET> --port 3000
```
Connects over an end-to-end encrypted QUIC tunnel directly into the car's gateway and serves the diagnostic dashboard on `http://localhost:3000`.

#### D. Model Context Protocol (MCP) Server for AI Agents
```bash
sterngate mcp
```
Communicates over standard input/output (JSON-RPC 2.0).

---

## 5. Mercedes-Benz 'Cascade of Death' Early Warning Watchdog

Automotive components in Mercedes-Benz vehicles (particularly W211, W219, W220, W204, W164) often fail in multi-stage cascading chains where a neglected $1.50–$160 wear item triggers multi-thousand-dollar catastrophic damage. Sterngate actively monitors **13 canonical failure chains**:

| # | Monitored Failure Chain | Root Cause Wear Item | Catastrophic Destruction ($$$$) | Detection Heuristics | Active Containment Strategy |
| :---: | :--- | :--- | :--- | :--- | :--- |
| **1** | **SBC Hydraulic Accumulator Exhaustion** | Nitrogen accumulator `A 000 430 26 94` ($120) | Pump motor burnout -> Total loss of power brake assist ($2,500) | Pre-charge $<70\text{ bar}$ (warning), $<55\text{ bar}$ (critical); pump duty cycle $\ge 75\%$ of pedal taps. | Accumulator pre-charge watchdog; bleeder calibration routines. |
| **2** | **Common Rail Injector 'Black Death'** | Copper crush washer `A 611 017 00 60` ($1.50) | Blow-by carbon cements injector into cylinder head; melts wiring harness ($1,800–$3,500) | Smooth-running balance $>+3.5\text{ mm}^3/\text{hub}$; rail pressure bleed down rate. | Early warning before carbon hardens; ceramic anti-seize service alert. |
| **3** | **722.6 Pilot Bushing ATF Wicking** | 13-pin connector O-rings `A 203 540 02 53` ($8) | Capillary wicking floods EGS52 TCU with ATF; shorts solenoid drivers ($1,500) | ATF temperature jump $>20^\circ\text{C}$ in $<5\text{s}$ with speed sensor jitter (`Y3/6n2`/`Y3/6n3`). | Diagnostic warning to replace $8 adapter plug before TCU board damage occurs. |
| **4** | **722.6 Torque Converter Lockup Clutch Slip** | PWM lockup solenoid `A 240 270 17 00` ($65) | Friction lining sheds into valve body spools and planetary gearsets ($2,800) | TCC slip $>30\text{ RPM}$ during commanded lockup in 3rd–5th gears. | Alerts to swap PWM solenoid before friction paper strips down to bare steel. |
| **5** | **DPF Differential Drift -> M55 Swirl Motor Short** | Drifting diff pressure sensor `A 006 153 95 28` ($45) | Backpressure blows turbo oil seal; oil pools onto M55 motor, blowing Fuse 54 ($3,200) | Flat pressure curve ($<15\text{ mbar}$ at $>3000\text{ RPM}$) or regen distance $>1000\text{ km}$. | Pressure plausibility guard prevents highway engine stalls. |
| **6** | **Camshaft Magnet Oil Wicking** | Cam solenoid seals `A 272 051 01 77` ($30) | Oil wicks through harness into Bosch ME9.7 ECU motherboard & O2 sensors ($2,400) | 5V reference bus dip with simultaneous O2 sensor heater resistance drift. | Prompts immediate installation of sacrificial blocking pigtails (`A 271 150 27 33`). |
| **7** | **Air Suspension Compressor Burnout (S211 ENR / W211 AIRMATIC)** | Leaking rear bellow `A 211 320 09 25` ($140) | Continuous run melts PTFE seal; $>30\text{A}$ current welds Hella relay closed ($1,200) | Continuous run $>40\text{s}$, drop rate $>4\text{ mm/h}$, duty cycle $>25\%$. | **Autonomous Thermal Watchdog & Software Kill-Switch** (Routine `0x0210`). |
| **8** | **ABC (Active Body Control) Hydraulic Surge** | Nitrogen damper sphere `A 220 327 02 15` (~$160) | Undamped 300+ bar shockwaves fracture tandem pump shaft and burst lines over hot exhaust ($8,000–$10,000) | Line pressure ripple $>15\text{ bar}$ (warning), $>25\text{ bar}$ (critical). | **Active ABC Limiter**: Software command via Routine `0x0220` (system pressure dump to 120 bar safe mode) + Routine `0x0221` (lock strut isolation valves). |
| **9** | **Electronic Steering Lock (ESL / ELV) DC Motor Seizure** | Johnson/Nichibo FC-280SC micro-motor brushes ($5) | Motor stalls mid-stroke, NEC micro blows security bit, Terminal 15/50 permanently inhibited ($2,000+) | Bolt unlock latency $>250\text{ms}$ (warning), $>500\text{ms}$ (critical lockout). | **Critical Directive**: Explicitly instructs owner **NOT TO REMOVE KEY** once latency threshold breaches, enabling plug-and-play emulator installation while unlocked. |
| **10** | **M272/M273 Balance Shaft & Idler Sprocket Wear** | Soft sintered drive sprocket `A 272 050 08 04` ($60) | Teeth grind smooth, chain skips timing, valve-to-piston collision ($6,000+) | Camshaft phase angle deviation $>1.5^\circ$ (warning), $>3.2^\circ$ (imminent jump) at hot idle ($80^\circ\text{C}$ coolant). | Timing phase deviation watchdog flags tooth erosion before permanent DTC 1200/1208 and collision. |
| **11** | **Valeo Radiator Glycol Intrusion into 722.6 Transmission** | Crimp seam defect in internal cooler ($0 part of radiator) | Glycol dissolves water-based glue on friction plates; linings peel into valve body ($3,500) | Harmonic TCC slip micro-oscillation ($4–12\text{ Hz}$, $>15\text{ RPM}$ warning, $>35\text{ RPM}$ critical). | Prompts immediate cuvette glycol test (`A 001 988 84 44`) and fitting an external air-to-oil transmission cooler. |
| **12** | **Cowl/Sunroof Drain Clog -> SAM Water Ingress** | Rubber duckbill drain valves clogged with leaves ($0 to clean) | Water overflows into SAM, electrolytic PCB corrosion, MOSFET bridge latches, 3–5A parasitic drain ($1,500/SAM) | Interior CAN-B bus sleep delay $>45\text{s}$ (warning), $>120\text{s}$ (critical) or standby parasitic current $>0.25\text{A}$. | **Deep Sleep Verification Guard**: Audits gateway sleep registers during shutdown and flags persistent wake-up loops. |
| **13** | **OM642 V-Valley Oil Cooler Seal Starvation** | Orange silicone seals `A 642 188 01 80` bake brittle ($4.50) | Highway high-speed oil depletion out bellhousing weep hole, spun rod bearings ($7,500+) | Highway dynamic oil level consumption rate $>0.10\text{ mm/100km}$ (warning), $>0.25\text{ mm/100km}$ (critical). | **Highway Oil Loss Alarm**: Detects rapid drop before dashboard warning lamp triggers, recommending purple Viton seals (`A 642 188 05 80`). |

```bash
# Evaluate live vehicle vitals against all 13 cascades
sterngate analyze cascades

# Evaluate custom telemetry JSON
sterngate analyze cascades --input custom_vitals.json
```

---

## 6. Active Hardware Safety Guards & Containment

Sterngate provides direct software containment routines allowing owners and technicians to protect vehicles from cascading failures while driving:

### A. S211 Air Suspension Compressor Protection
```bash
# Inhibit compressor to prevent motor burnout & relay welding during leaks (Routine 0x0210)
sterngate analyze suspension --inhibit

# Set suspension into Workshop / Transport Mode (Routine 0x0211)
sterngate analyze suspension --workshop

# Restore normal automatic pneumatic self-leveling (Routine 0x0212)
sterngate analyze suspension --restore
```

### B. ABC (Active Body Control) Hydraulic Surge Limiter
```bash
# Dump system pressure to 120 bar safe fallback to protect lines & pump from 300+ bar surges (Routine 0x0220)
sterngate analyze abc --dump

# Lock strut level isolation valves to contain fluid loss and line burst over exhaust (Routine 0x0221)
sterngate analyze abc --lock

# Restore normal active dynamic body control (Routine 0x0222)
sterngate analyze abc --restore
```

---

## 7. Vehicle Garage & Git Configuration Rollbacks

Every vehicle scanned by Sterngate is saved into `data/vehicles/<VIN>/`:
- Automatically decodes the VIN (manufacturer, model, chassis, powertrain, country of origin).
- Maintains a dedicated Git repository tracking diagnostic quick scan results, live vitals history, and variant coding changes (`coding/<MODULE>.coding.hex`).
- Allows instant forensic rollback of coding parameters using standard Git semantics:
```bash
# Perform a comprehensive gateway quick scan and commit to garage
sterngate diag scan --save

# List all tracked garage vehicles
sterngate diag garage

# Inspect configuration history and git log for a vehicle
sterngate diag garage --vin WDB2112061A892341
```

---

## 8. In-Flight Drive Telemetry & A/B Benchmark Analysis

Sterngate computes instantaneous fuel consumption rate ($L/100\text{km}$) and compares two drive logs to verify if maintenance, new solenoids, or tuning modifications were beneficial:
```bash
# Run A/B comparative benchmark between two drive telemetry runs
sterngate analyze compare
```

---

## 9. Daimler 1,340+ ECU Diagnostic Catalog Explorer

Sterngate features an expanded diagnostic routing catalog (`data/ecu_catalog.json`) indexing 1,347 canonical electronic control units across Mercedes-Benz architectures:
```bash
# View summary statistics of the ECU catalog
sterngate ecu stats

# Search for ECUs by name or chassis platform
sterngate ecu search EGS
sterngate ecu search W223

# Inspect detailed diagnostic routing for an ECU
sterngate ecu inspect MED1775
sterngate ecu inspect ESP223
```

---

## 10. Bus Discovery, Flashing Suite & Workshop Service Routines

### A. Bus Discovery & Automated Profile Generation
Interrogate uncataloged vehicle networks without proprietary engineering tools:
```bash
# Probes CAN IDs (0x7E0..0x7EF), reads identification DIDs, correlates with 1,340+ ECU database
sterngate diag discover --start 0x7E0 --end 0x7EF

# Automatically compile discovered ECUs and standard DIDs into a vehicle profile
sterngate profile generate --name "w211_custom_om646" --out profiles/custom.json
```

### B. Direct Terminal ECU Flashing Suite
Safely stage and flash ECU calibration binaries from the terminal with strict hardware and voltage interlocks:
```bash
# 1. Stage and cryptographically verify ROM binary
sterngate flash stage --module EDC16 --file calibration.bin \
  --hw-id 0281012234 --sw-id 1037372120 --start-address 0x00040000

# 2. Pre-flight verification (battery voltage >= 12.5V, SHA-256, Bosch CRC32)
sterngate flash preflight --manifest /var/run/sterngate/flash_manifest.json

# 3. Start detached flash sequence (with interactive safety confirmation or --yes)
sterngate flash start --manifest /var/run/sterngate/flash_manifest.json --yes

# 4. Monitor live progress of detached flashing worker
sterngate flash status
```

### C. Workshop Service Routines
Perform safety-critical actuations that previously required Mercedes Xentry/DAS:
```bash
# SBC (Sensotronic Brake Control) Brake Pad Replacement Mode:
# Dumps 160 bar accumulator into reservoir, retracts pistons, locks wake-up triggers
sterngate service sbc --action deactivate
# Reactivate and run high-pressure self-bleed check
sterngate service sbc --action reactivate

# Common Rail Injector IMA (Injector Quantity Adaptation) Coding:
# Read 6/7-character production tolerance compensation code
sterngate service ima --cylinder 1
# Write IMA code and record git commit in vehicle's garage history
sterngate service ima --cylinder 1 --code 7B8HNA --vin WDB2112061A000001

# Air Suspension Corner Actuation & Sensor Calibration
sterngate service suspension --corner rear-left --action inflate
sterngate service suspension --corner all --action calibrate-zero
```

### D. Variant Coding CLI & Git Garage History
```bash
# Read hex coding string from target module
sterngate coding read --module EDC16 --did 0x0100

# Write verified coding string with CommandEnvelope zero-trust gate
sterngate coding write --module EDC16 --did 0x0100 --data 01020304 --vin WDB2112061A000001 --note "Disable EGR"

# Backup all module codings into vehicle git garage
sterngate coding backup --vin WDB2112061A000001

# Diff vehicle coding against prior git revisions
sterngate coding diff --vin WDB2112061A000001
```

### E. Standalone High-Resolution HTML Diagnostic Report
Generate a self-contained, beautifully styled HTML diagnostic report for print or customer handoff:
```bash
sterngate diag scan --export-html report.html --lang en
```

---

## 11. Binary ROM Map Scanning, Stage 1/2 Tuning & Bosch Checksums (`sterngate tune`)

Sterngate features an automated calibration engine capable of scanning raw ROM binaries, synthesizing Stage 1/2 tunes, zeroing DTC error switches, and recalculating Bosch MPC5xx 32-bit block checksums:

```bash
# 1. Scan binary ROM dump for calibration maps, hardware IDs, and checksum blocks
sterngate tune scan --rom stock_edc16.bin

# 2. Synthesize verified Stage 1 tune (+18% torque, +120 mbar boost, +50 bar rail, stock emissions)
sterngate tune stage1 --rom stock_edc16.bin \
  --chassis "W211 E280 CDI" --ecu EDC16CP31 --output stage1.sgmod --armor

# 3. Synthesize Stage 2 tune (+25% torque, DPF delete, EGR zeroed, P0401/P2002 suppressed)
sterngate tune stage2 --rom stock_edc16.bin \
  --chassis "W211 E280 CDI" --ecu EDC16CP31 --output stage2.sgmod --armor

# 4. Precision DTC Suppression (zeroes single-byte error enable switches in ROM table)
sterngate tune dtc-kill --rom stock_edc16.bin --codes P0401,P2002 \
  --output dtc_delete.sgmod --armor

# 5. Verify and recalculate Bosch MPC5xx 32-bit partitioned block checksums
sterngate tune checksum --rom modified_rom.bin --fix --output fixed_rom.bin
```

> **Phase 0 state:** `stage1`, `stage2` and `dtc-kill` currently refuse on every ROM — no flash patch may be minted for a map that was not located by the detector, and only the SVBL is located today. See `.agents/skills/ecu-tuning/SKILL.md` for the current detector state.

---

## 12. Shareable Community Mod Packages (`.sgmod`) & Reed-Solomon Parity (`sterngate mod`)

Sterngate packages vehicle calibrations, DID configurations, and flash patches into portable `.sgmod` files protected by Reed-Solomon $GF(2^8)$ error-correction parity:

```bash
# 1. Inspect mod package compatibility, target vehicle rules, and test FEC self-healing
sterngate mod inspect mods/w211_top_speed_300.sgmod

# 2. Safely apply mod to vehicle with automatic pre-mod Git garage snapshotting
sterngate mod apply mods/w211_top_speed_300.sgmod --vin WDB2112061A000001

# 3. Author a new shareable community mod package
sterngate mod create --name "EGR Airmass Offset" --author "TunerKim" \
  --description "Increases fresh air mass by 40mg to minimize intake manifold soot" \
  --chassis W211 --ecu EDC16 --did 0x0115 --data "0028" \
  --output mods/egr_offset.sgmod --armor

# 4. List all installed mod packages in local library
sterngate mod library
```

---

## 13. Donor ECU Re-VIN Adaptation & One-Click Guided Workflows

Sterngate provides automated, guided workshop procedures that replace legacy Xentry engineering menus:

### Donor Replacement ECU Re-VIN Adaptation
Seamlessly pair salvage or replacement ECUs to the vehicle:
```bash
# Unlocks seed-key SecurityAccess, rewrites VIN (0xF190), verifies readback, commits to Git garage
sterngate coding revin --ecu CR4 --vin WDB2112061A999888
```

### One-Click Guided Workshop Procedures
```bash
# Configure VMax top speed limiter (e.g. 250 km/h)
sterngate service workflow vmax --speed 250

# Mute instrument cluster seatbelt acoustic warning chime
sterngate service workflow seatbelt --disable

# Enable instrument cluster exact remaining fuel in liters (Restliteranzeige)
sterngate service workflow tank-liters --enable

# Enable front SAM cornering fog lights (Abbiegelicht)
sterngate service workflow cornering --enable

# Reset AdBlue / SCR emergency lockout counter and NOx adaptations
sterngate service workflow adblue-reset

# Optimize Common Rail EGR soot reduction offset (+40mg fresh air)
sterngate service workflow egr-optimize
```

---

## Tactrix OpenPort 2.0 Linux Setup (Non-Root USB Access)

Sterngate communicates directly with Tactrix OpenPort 2.0 bulk endpoints on Linux without root/sudo:

```bash
# 1. Install udev permissions rule
sudo cp scripts/99-tactrix-openport.rules /etc/udev/rules.d/

# 2. Reload and trigger udev
sudo udevadm control --reload-rules && sudo udevadm trigger

# 3. Ensure your user is in the plugdev group
sudo usermod -aG plugdev $USER
```

For complete technical documentation on the reverse-engineered AT-command syntax and binary packet structure, see [docs/TACTRIX_PROTOCOL.md](docs/TACTRIX_PROTOCOL.md).

---

## OBD-II Port Wiring Reference

For Mercedes W211/S211 and most ISO 15765-4 compliant vehicles:

| OBD-II Pin | Signal Description | Hardware Connection |
| :--- | :--- | :--- |
| **Pin 4** | Chassis Ground | Ground (GND) |
| **Pin 5** | Signal Ground | Ground (GND) |
| **Pin 6** | CAN High (CAN-D Diagnostics) | CAN_H (500 kbps) |
| **Pin 14** | CAN Low (CAN-D Diagnostics) | CAN_L (500 kbps) |
| **Pin 16** | Battery Power (+12V Continuous) | Power Input / OpenPort ADC Sensing |

---

## Legal Disclaimer & License

- **Disclaimer**: Sterngate is distributed strictly on an **"AS IS"** basis, **WITHOUT WARRANTY OF ANY KIND**. By using this software, you agree that you do so **AT YOUR OWN RISK**, and that the authors and contributors bear **ZERO LIABILITY** for bricked ECUs, immobilized vehicles, or mechanical damages. See [DISCLAIMER.md](DISCLAIMER.md) for the complete legal notice.
- **License**: Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.


