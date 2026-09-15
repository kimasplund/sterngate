---
name: sterngate-ops
description: >-
  Operational runbook for running, testing, and troubleshooting Sterngate in various modes:
  local SBC dashboard, client car-side P2P bridge, remote technician server, and virtual simulation.
---

# Sterngate Operations Runbook

Use this skill when building, starting, or verifying Sterngate in any of its operational modes.

## 1. Quick Mode Selection

| Mode | Command | Alias / Shortcut | Use Case | Requirements |
| :--- | :--- | :--- | :--- | :--- |
| **Local** | `sterngate --local` | - | In-car standalone SBC (Raspberry Pi / laptop) serving Web UI on Wi-Fi | Local CAN adapter (`can0` or `--openport`) |
| **Bridge (Host)** | `sterngate --bridge` | `--car` | In-car gateway bridge (P2P Host / DoIP Server) generating ticket and listening | Local CAN adapter + internet |
| **Tech (Client)** | `sterngate --tech --ticket <TICKET>` | `--ticket <TICKET>` | Remote technician workstation (Tester / Dialer) dialing car node | Internet access |
| **Mock** | `sterngate mock` | - | Offline development and CI/CD testing with simulated W211 | None (zero hardware needed) |
| **MCP** | `sterngate mcp` | - | Running as a Model Context Protocol tool provider for AI agents | stdio |

---

## 2. Setting Up Virtual CAN (`vcan0`) for Testing

When testing on a Linux machine without a physical CAN transceiver:

```bash
# Enable the kernel vcan module
sudo modprobe vcan

# Create and bring up the virtual CAN interface
sudo ip link add dev vcan0 type vcan
sudo ip link set up vcan0

# Verify interface is UP
ip link show vcan0
```

To run Sterngate against `vcan0`:
```bash
cargo run --package sterngate-cli -- --local --can-interface vcan0 --port 8080
```

> [!IMPORTANT]
> The dashboard binds to `127.0.0.1` by default. The diagnostic API has no
> authentication and can actuate the vehicle (SBC depressurisation, compressor
> inhibit, variant coding, flashing), so only expose it deliberately:
> ```bash
> # Reachable from other devices on the workshop network - do this knowingly
> cargo run --package sterngate-cli -- --local --can-interface can0 --bind 0.0.0.0 --port 8080
> ```
> For remote access prefer `--bridge` / `--tech`, which tunnel over encrypted
> Iroh P2P instead of exposing the listener.

---

## 3. Running Standalone Mock Simulation

If you do not have root access or kernel vcan support, launch the built-in mock simulator:
```bash
cargo run --package sterngate-cli -- mock --port 8080
```
This spawns the internal `VirtualCanInterface`, simulating a Mercedes-Benz W211 with live OM646 engine and 722.6 transmission telemetry. You can open `http://localhost:8080` in your browser to inspect live gauges, read DTCs, and test variant coding.

---

## 4. P2P Remote Diagnostics Flow

### Step A: Car-Side Vehicle Bridge (P2P Host)
```bash
# With SocketCAN adapter
sterngate --bridge --can-interface can0

# Or with Tactrix OpenPort 2.0
sterngate --bridge --openport
```
Output:
```
=============================================================
  Starting Sterngate: Car-Side Diagnostic Bridge (P2P Host)
  Role: On-Vehicle Gateway Bridge & Ticket Host
=============================================================
>>> SHARE THIS TICKET WITH YOUR REMOTE TECHNICIAN <<<
node:abc123456...
```

### Step B: Remote Technician Client (P2P Dialer)
```bash
# Explicit tech mode
sterngate --tech --ticket node:abc123456... --port 3000

# Or simply pass the ticket directly
sterngate --ticket node:abc123456... --port 3000
```
The technician's browser opens at `http://localhost:3000`, connected over an end-to-end encrypted QUIC tunnel directly to the vehicle.

---

---

## 5. Routine Diagnostics & Flight Recording

### Triggering UDS Routines (Service 0x31) via CLI
```bash
# Prime fuel pump & rail bleed (0xFF01)
sterngate diag routine --routine 0xFF01 --module EDC16

# Reset zero-quantity injector adaptations (NMK, 0x0201)
sterngate diag routine --routine 0x0201 --module EDC16

# Trigger DPF service regeneration (0x0202)
sterngate diag routine --routine 0x0202 --module EDC16
```

### Continuous Flight Telemetry Logging (REST API)
```bash
# Start 10-50Hz continuous flight recording
curl -s -X POST http://localhost:8080/api/v1/recorder/start -H "Content-Type: application/json" -d '{"filename": "dyno_stage2.csv"}' | jq .

# Query flight recorder status (rows captured, elapsed time)
curl -s http://localhost:8080/api/v1/recorder/status | jq .

# Stop flight recorder and flush CSV to disk
curl -s -X POST http://localhost:8080/api/v1/recorder/stop | jq .
```

---

## 6. Multilingual Diagnostics & i18n

Sterngate supports multilingual operation across the Web UI, REST API, and CLI (`en`, `de`, `sv`).
- German (`de`) provides authentic Daimler OEM terminology (*Getriebeöltemperatur*, *Nullmengenkalibrierung*, *Wandlerüberbrückungskupplung*).
- English (`en`) is the default international automotive standard.
- Swedish (`sv`) provides full Scandinavian regional support.

### Multilingual CLI Commands
```bash
# Read DTCs in German or Swedish
sterngate diag dtc --module EDC16 --lang de
sterngate diag dtc --module EDC16 --lang sv

# Execute UDS Routine with localized feedback
sterngate diag routine --routine 0xFF01 --module EDC16 --lang de
sterngate diag routine --routine 0x0201 --module EDC16 --lang sv
```

### Multilingual REST API
```bash
# Fetch available UI and diagnostic locales
curl -s http://localhost:8080/api/v1/locales | jq .

# Fetch DTCs in German
curl -s "http://localhost:8080/api/v1/dtc?lang=de" | jq .

# Execute routine with localized completion response
curl -s -X POST http://localhost:8080/api/v1/routine \
  -H "Content-Type: application/json" \
  -d '{"module": "EDC16", "routine_id_hex": "0xFF01", "sub_function": 1, "lang": "de"}' | jq .
```

---

## 7. ECU Catalog & Diagnostic Routing

Sterngate maintains a 21st-century compact diagnostic routing index of 990 canonical ECUs (`data/ecu_catalog.json`), mapping protocols (UDS/KWP2000), physical Tx/Rx CAN IDs, functional IDs, DTC counts, and multi-chassis platforms without requiring legacy binary files.

### CLI Operations
```bash
# View summary statistics of the ECU catalog
sterngate ecu stats

# Search for ECUs by name or chassis platform
sterngate ecu search EGS
sterngate ecu search W211

# Inspect detailed diagnostic routing for an ECU
sterngate ecu inspect EGS52
sterngate ecu inspect VGSNAG2
```

### REST API Operations
```bash
# Fetch catalog statistics
curl -s http://localhost:8080/api/v1/ecu/stats | jq .

# Search ECUs via REST query
curl -s "http://localhost:8080/api/v1/ecu/search?q=EGS&limit=5" | jq .

# Inspect single ECU definition
curl -s http://localhost:8080/api/v1/ecu/inspect/EGS52 | jq .
```

---

## 8. Vehicle Quick Scan & Comprehensive Health Reports

Sterngate can perform an automated diagnostic quick scan across all gateway ECUs (`EDC16`, `EGS52`, `SBC`, `ENR`, `CGW`), decode the vehicle's VIN (identifying chassis like S211 Estate and OM646 engines), query hardware/software part numbers, read fault codes, and generate an issue report.

### CLI Quick Scan
```bash
# Interrogate all vehicle ECUs and print human-readable summary
sterngate diag scan

# Scan and output formatted Markdown diagnostic report
sterngate diag scan --report

# Scan and automatically synchronize vehicle into Git garage
sterngate diag scan --save-vehicle --lang de
```

### REST API Quick Scan
```bash
curl -s -X POST http://localhost:8080/api/v1/vehicle/scan \
  -H "Content-Type: application/json" \
  -d '{"lang": "en", "save_to_garage": true}' | jq .
```

---

## 9. Per-Car Git Versioned Garage & Rollbacks

Sterngate manages an autonomous Git repository for every connected vehicle under `data/vehicles/<VIN>/`. Every quick scan, variant coding session, and adaptation reset creates an atomic Git commit preserving the exact hexadecimal bytes (`coding/<MODULE>.coding.hex`) and human-readable parameter configurations.

### CLI Garage Management
```bash
# List all tracked vehicles in the garage
sterngate vehicle list

# Inspect hardware, installed ECUs, and last scan data for a vehicle
sterngate vehicle inspect WDB2112061A892341

# View Git commit history for a vehicle
sterngate vehicle history WDB2112061A892341

# Atomically roll back variant coding to a previous commit
sterngate vehicle rollback WDB2112061A892341 82c43e2
```

### REST API Garage Endpoints
```bash
# List all tracked vehicles
curl -s http://localhost:8080/api/v1/vehicles | jq .

# Inspect vehicle record
curl -s http://localhost:8080/api/v1/vehicles/WDB2112061A892341 | jq .

# View Git commit history
curl -s http://localhost:8080/api/v1/vehicles/WDB2112061A892341/history | jq .
```

---

## 10. Predictive Suspension & Drive Analytics

### S211 Rear Air Suspension (ENR) & AIRMATIC Leak Detection
Detects pneumatic leaks on the Mercedes S211 Estate / W211 AIRMATIC before compressor burnout by evaluating:
- Stationary height drop rate ($> 10\text{ mm/h}$ critical leak)
- Left vs. Right rear asymmetry ($> 20\text{ mm}$ sensor calibration issue)
- Continuous compressor runtime ($> 45\text{s}$ warning / $> 60\text{s}$ thermal cutout risk)
- Duty cycle ($> 25\%$ strain warning)

```bash
# Evaluate pneumatic suspension health
sterngate analyze suspension

# Active Compressor Protection (Kill-switch / Safe Mode)
# Inhibit compressor to prevent motor burnout & relay welding during leaks
sterngate analyze suspension --inhibit

# Set suspension to Workshop / Transport Mode (fixed height for driving/towing)
sterngate analyze suspension --workshop

# Restore normal automatic pneumatic leveling
sterngate analyze suspension --restore

# REST API call - Diagnostic Analysis
curl -s -X POST http://localhost:8080/api/v1/analyze/suspension \
  -H "Content-Type: application/json" -d '{}' | jq .

# REST API call - Compressor Protection Control
curl -s -X POST http://localhost:8080/api/v1/suspension/compressor/control \
  -H "Content-Type: application/json" \
  -d '{"action": "inhibit", "reason": "Airbag leak detected"}' | jq .

# REST API call - Compressor Protection Status
curl -s http://localhost:8080/api/v1/suspension/compressor/status | jq .
```

### In-Flight Drive Telemetry & A/B Benchmark Comparison
Computes real-time instant diesel fuel rate ($L/h$) and consumption ($L/100\text{km}$) using OM646 4-stroke displacement mathematics. Compares Baseline vs. Modified runs to determine if parameter or hardware changes were beneficial:

```bash
# Run A/B comparative benchmark between two drive telemetry runs
sterngate analyze compare

# REST API call
curl -s -X POST http://localhost:8080/api/v1/analyze/compare \
  -H "Content-Type: application/json" \
  -d '{
    "run_a": {"duration_seconds": 1800, "distance_km": 35, "average_speed_kmh": 70, "average_consumption_l_per_100km": 7.6, "average_rpm": 1950, "max_boost_hpa": 1450, "average_rail_pressure_bar": 1150, "average_tcc_slip_rpm": 38, "final_coolant_temp_c": 78, "seconds_to_reach_85c": null},
    "run_b": {"duration_seconds": 1800, "distance_km": 35, "average_speed_kmh": 70, "average_consumption_l_per_100km": 6.9, "average_rpm": 1900, "max_boost_hpa": 1480, "average_rail_pressure_bar": 1140, "average_tcc_slip_rpm": 8, "final_coolant_temp_c": 88, "seconds_to_reach_85c": 420}
  }' | jq .
```

### Active ABC (Active Body Control) Hydraulic Surge Limiter
Protects tandem pumps and hydraulic lines against undamped 300+ bar pressure shockwaves when the nitrogen pulsation damper sphere (`A 220 327 02 15`) fails:

```bash
# Actuate pressure fallback dump to 120 bar safe mode (Routine 0x0220)
sterngate analyze abc --dump

# Lock strut level isolation valves to contain fluid loss and line rupture (Routine 0x0221)
sterngate analyze abc --lock

# Restore normal active dynamic body control (Routine 0x0222)
sterngate analyze abc --restore

# REST API call - ABC Limiter Control
curl -s -X POST http://localhost:8080/api/v1/abc/control \
  -H "Content-Type: application/json" \
  -d '{"action": "dump"}' | jq .
```

### Mercedes-Benz 'Cascade of Death' Early Warning System
Detects inexpensive $2–$160 failing wear items before they snowball into $2,500–$10,000+ destroyed ECUs, welded relays, line fires, or engine replacements:
1. **SBC Hydraulic Accumulator Exhaustion**: Accumulator pre-charge pressure ($<70\text{ bar}$ warning / $<55\text{ bar}$ critical) and pump duty cycle per brake application.
2. **Common Rail Injector 'Black Death'**: Cylinder smooth-running balance ($>+3.5\text{ mm}^3/\text{hub}$) to prevent carbon cementing and harness melting.
3. **722.6 Pilot Bushing ATF Wicking**: ATF temperature spikes ($>20^\circ\text{C}$ jump) and speed sensor jitter to prevent EGS52 TCU flooding.
4. **722.6 TCC Lockup Clutch Shredding**: TCC slip ($>30\text{ RPM}$) during commanded lockup to prevent valve body abrasion.
5. **DPF Differential Drift -> M55 Swirl Motor Short**: Flat pressure curve under boost to prevent turbo oil blow-by and blown Fuse 54.
6. **Camshaft Magnet Oil Wicking**: 5V sensor reference dip + O2 heater faults to prevent engine ECU oiling.
7. **Air Suspension Compressor Burnout**: Continuous runtime ($>40\text{s}$) to prevent piston seal melting and relay welding.
8. **ABC Pulsation Damper Surge**: Hydraulic line ripple ($>15\text{ bar}$ warning / $>25\text{ bar}$ critical) to prevent tandem pump shaft shear and exhaust line fires.
9. **Electronic Steering Lock (ESL / ELV) Brush Seizure**: Unlock duration ($>250\text{ms}$ warning / $>500\text{ms}$ critical) instructing owner NOT to remove key before emulator install.
10. **M272/M273 Balance Shaft & Idler Sprocket Wear**: Camshaft phase angle deviation ($>1.5^\circ$ warning / $>3.2^\circ$ critical) before chain skips teeth.
11. **Valeo Radiator Glycol Intrusion**: Harmonic TCC slip micro-oscillation ($4–12\text{ Hz}$, $>15\text{ RPM}$) before clutch paper delamination.
12. **Cowl/Sunroof Drain Clog -> SAM Water Ingress**: CAN-B bus sleep failure ($>45\text{s}$ warning / $>120\text{s}$ critical) and quiescent drain ($>0.25\text{A}$) before PCB bridge corrosion.
13. **OM642 V-Valley Oil Cooler Seal Starvation**: Dynamic highway oil loss ($>0.10\text{ mm/100km}$ warning / $>0.25\text{ mm/100km}$ critical) before rod bearing starvation.

```bash
# Evaluate vehicle vitals against all 13 cascades
sterngate analyze cascades

# REST API call - Live telemetry evaluation
curl -s http://localhost:8080/api/v1/analyze/cascades | jq .

# REST API call - Custom telemetry evaluation
curl -s -X POST http://localhost:8080/api/v1/analyze/cascades \
  -H "Content-Type: application/json" \
  -d '{"sbc_accumulator_pressure_bar": 52.0, "abc_pressure_ripple_bar": 28.0}' | jq .
```

---

## 11. Workshop Service Routines, Bus Discovery & Flashing CLI

### A. Bus Discovery & Automated Profile Generation
```bash
# Probes CAN bus IDs (0x700..0x7EF) and interrogates responsive ECUs
sterngate diag discover --start 0x7E0 --end 0x7EF

# Automatically generate declarative JSON profile matching catalog
sterngate profile generate --name "mercedes_custom_w211" --out profiles/custom.json
```

### B. Workshop Service Routines
```bash
# SBC Brake Pad Replacement Mode (0 bar pressure dump & finger safety lock)
sterngate service sbc --action deactivate
# Reactivate and run hydraulic high-pressure self-bleed check
sterngate service sbc --action reactivate

# Read Common Rail Injector IMA production calibration codes
sterngate service ima --cylinder 1
# Write IMA code and record git commit in vehicle garage
sterngate service ima --cylinder 1 --code 7B8HNA --vin WDB2112061A000001

# Air Suspension Corner Actuation & Calibration
sterngate service suspension --corner rear-left --action inflate
sterngate service suspension --corner all --action calibrate-zero

# Search 1,523+ cataloged OEM workshop service & actuator routines (0x31)
sterngate service list --query steering --limit 10
sterngate service list --ecu CR4

# Execute generic workshop service routine by ID
sterngate service run 0x0305 --ecu CR4 --data 01FF
```

### C. Variant Coding & Git Version Tracking
```bash
# Search 3,155+ cataloged factory Variant Coding DIDs (0x2E)
sterngate coding list-dids --query vin --limit 10
sterngate coding list-dids --ecu EDC16

# Read hex coding string from ECU
sterngate coding read --module EDC16 --did 0x0100

# Write coding string with command envelope protection & git commit
sterngate coding write --module EDC16 --did 0x0100 --data 01020304 --vin WDB2112061A000001 --note "Speed limiter 250 km/h"

# Donor replacement ECU Re-VIN Adaptation (SecurityAccess unlock, 0x2E write, verification, git commit)
sterngate coding revin --ecu CR4 --vin WDB2112061A999888

# Backup all module codings into vehicle git garage
sterngate coding backup --vin WDB2112061A000001

# Diff current vehicle configuration against previous git commit
sterngate coding diff --vin WDB2112061A000001
```

### D. HTML Diagnostic Report Export
```bash
# Perform multi-ECU quick scan and export self-contained HTML report
sterngate diag scan --export-html report.html --lang en
```

### E. Mercedes One-Click Quick Mods (REST API & Web UI)
Pre-tested configuration recipes with automated Git garage history tracking:
```bash
# 1. VMax Speed Limiter (DID 0x0110)
curl -s -X POST http://localhost:8080/api/v1/workflow/vmax \
  -H "Content-Type: application/json" \
  -d '{"speed_limit_kmh": 250, "vin": "WDB2112061A000001"}' | jq .

# 2. Seatbelt Acoustic Warning Chime (DID 0x0201)
curl -s -X POST http://localhost:8080/api/v1/workflow/seatbelt-chime \
  -H "Content-Type: application/json" \
  -d '{"acoustic_enabled": false, "vin": "WDB2112061A000001"}' | jq .

# 3. Remaining Fuel in Liters / Restliteranzeige (DID 0x0205)
curl -s -X POST http://localhost:8080/api/v1/workflow/tank-liters \
  -H "Content-Type: application/json" \
  -d '{"enabled": true, "vin": "WDB2112061A000001"}' | jq .

# 4. Front SAM Cornering Fog Lights / Abbiegelicht (DID 0x0310)
curl -s -X POST http://localhost:8080/api/v1/workflow/cornering-lights \
  -H "Content-Type: application/json" \
  -d '{"enabled": true, "vin": "WDB2112061A000001"}' | jq .
```

### F. Local Firmware Vault & Upgrade Scanner (REST API)
```bash
# The vault root is configured once, in precedence order:
#   --vault <DIR>  >  STERNGATE_VAULT_ROOT  >  ./firmware_vault
# Both vault routes are confined to that root, so `path` and `file_path` are
# interpreted inside it and anything resolving outside (../, absolute paths,
# symlinks) is rejected with 400.

# Scan the whole vault (blank path), or pass a subfolder inside it
curl -s "http://localhost:8080/api/v1/vault/scan?path=&hw_id=0281012224&sw_id=1037372332" | jq .
curl -s "http://localhost:8080/api/v1/vault/scan?path=w211/edc16" | jq .

# Stage firmware binary for detached flashing sequence.
# The server measures voltage itself via VehicleInterface::measure_battery_voltage.
# measured_voltage is optional when the adapter can measure (e.g. OpenPort Pin 16)
# — supplying one that disagrees with the adapter reading by more than 1.0 V is
# refused. It is required only when the adapter cannot measure (SocketCAN, mock).
curl -s -X POST http://localhost:8080/api/v1/vault/stage \
  -H "Content-Type: application/json" \
  -d '{"file_path": "W211_OM646_Stage1.bin", "measured_voltage": 13.4}' | jq .
# file_path is resolved inside the vault root, so it is given relative to it
# (an absolute path is accepted only while it still resolves inside the root).
```

---

## 12. Verification Checklist

1. **Verify Binary Compiles**:
   ```bash
   cargo build --workspace
   ```
2. **Run All Integration Tests**:
   ```bash
   cargo test --workspace
   ```
3. **Verify API Endpoints**:
   ```bash
   curl -s http://localhost:8080/api/v1/locales | jq .
   curl -s "http://localhost:8080/api/v1/dtc?lang=de" | jq .
   curl -s http://localhost:8080/api/v1/telemetry | jq .
   curl -s http://localhost:8080/api/v1/vehicle/scan | jq .
   curl -s http://localhost:8080/api/v1/vehicles | jq .
   curl -s http://localhost:8080/api/v1/analyze/suspension | jq .
   curl -s http://localhost:8080/api/v1/service/ima | jq .
   curl -s http://localhost:8080/api/v1/diag/report.html -o report.html
   ```

---

## 13. Native Tactrix OpenPort 2.0 Hardware Operation

Sterngate includes a reverse-engineered native Linux USB driver for Tactrix OpenPort 2.0 cables (`VID 0x0403`, `PID 0xCC4D` / `0xCC4C`).

### Key Features
* **Zero Windows Dependencies**: Communicates directly over USB bulk endpoints via `rusb`.
* **100% Clone Safe**: Strips out all vendor phone-home and anti-clone flash erase routines that brick Chinese clones on Windows.
* **Pin 16 Hardware ADC**: Directly reads real vehicle battery millivolts to enforce the $\ge 12.5\text{ V}$ flashing interlock.

### Setup udev Rules (Non-Root USB Access)
```bash
sudo cp scripts/99-tactrix-openport.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG plugdev $USER
```

### Usage Examples
```bash
# Run local dashboard with Tactrix OpenPort 2.0
sterngate --local --openport

# Run live diagnostic telemetry
sterngate --openport diag live

# voltage is measured through the adapter; --voltage only overrides a dry-run preflight
sterngate --openport flash preflight --manifest flash_pkg/edc16_stage1.json --rom flash_pkg/edc16_stage1.bin
```

`sterngate flash start` always reads the adapter's own measurement and refuses outright on interfaces that cannot measure (SocketCAN, mock) — there is no `--voltage` override for a real flash, only for `flash preflight`'s dry run. Once the Flashing State Machine leaves `Idle`, every interface-touching route (diagnostics, coding, service, vault, mods) answers `HTTP 423 Locked` until the flash finishes.

---

## 14. Binary ROM Calibration, Stage Tuning & DTC Suppression (`sterngate tune`)

Sterngate includes an automated ECU map detection, stage tune generator, and Bosch MPC5xx block checksum engine:

### CLI Commands
```bash
# 1. Scan binary ROM dump for calibration maps, hardware IDs, and checksum blocks
sterngate tune scan --rom /path/to/stock_edc16.bin

# 2. Generate verified Stage 1 calibration package (+18% torque, +120 mbar boost, +50 bar rail)
sterngate tune stage1 --rom /path/to/stock_edc16.bin \
  --chassis "W211 E280 CDI" --ecu EDC16CP31 --output stage1.sgmod --armor

# 3. Generate Stage 2 calibration package (+25% torque, DPF Off, EGR zeroing, P0401/P2002 suppressed)
sterngate tune stage2 --rom /path/to/stock_edc16.bin \
  --chassis "W211 E280 CDI" --ecu EDC16CP31 --output stage2.sgmod --armor

# 4. Suppress specific Diagnostic Trouble Codes (zero error enable switches in ROM table)
sterngate tune dtc-kill --rom /path/to/stock_edc16.bin --codes P0401,P2002 \
  --output dtc_delete.sgmod --armor

# 5. Verify and recalculate Bosch MPC5xx 32-bit block checksums
sterngate tune checksum --rom /path/to/modified_rom.bin --fix --output /path/to/fixed_rom.bin
```

> **Phase 0 state:** commands 2–4 (`stage1`, `stage2`, `dtc-kill`) currently refuse on every ROM until the detector rebuild locates real maps; only `scan` and `checksum` are fully functional today. See `.agents/skills/ecu-tuning/SKILL.md`.

### REST API Endpoints
```bash
# These routes take the ROM in the request body only. They deliberately accept
# no filesystem path: the HTTP API is reachable by any client that can open the
# port, so a caller-supplied path would be an arbitrary file read/write.
ROM_B64=$(base64 -w0 /path/to/stock.bin)

# Scan ROM
curl -s -X POST http://localhost:8080/api/v1/tuning/scan \
  -H "Content-Type: application/json" \
  -d "{\"rom_base64\": \"$ROM_B64\"}" | jq .

# Generate Stage 1 Tune
curl -s -X POST http://localhost:8080/api/v1/tuning/stage1 \
  -H "Content-Type: application/json" \
  -d "{\"rom_base64\": \"$ROM_B64\", \"chassis\": \"W211 E280 CDI\", \"ecu_name\": \"EDC16CP31\"}" | jq .

# Suppress DTC Error Masks
curl -s -X POST http://localhost:8080/api/v1/tuning/dtc/kill \
  -H "Content-Type: application/json" \
  -d "{\"rom_base64\": \"$ROM_B64\", \"p_codes\": [\"P0401\", \"P2002\"]}" | jq .

# Verify & Recalculate Checksums (corrected ROM is returned as fixed_base64)
curl -s -X POST http://localhost:8080/api/v1/tuning/checksum/fix \
  -H "Content-Type: application/json" \
  -d "{\"rom_base64\": \"$ROM_B64\"}" | jq -r .fixed_base64 | base64 -d > /path/to/fixed.bin
```

---

## 15. Community Mod Packages & Reed-Solomon Parity (`sterngate mod`)

Sterngate allows packaging variant coding calibrations, DID patches, and flash modifications into shareable `.sgmod` packages protected by Reed-Solomon $GF(2^8)$ error-correction:

### CLI Commands
```bash
# 1. Inspect mod package compatibility, target vehicle rules, and test FEC self-healing
sterngate mod inspect mods/w211_top_speed_300.sgmod

# 2. Safely apply mod to vehicle with automatic pre-mod Git garage snapshotting
# --vin is required; voltage is read from the OpenPort ADC (refused on can0/mock)
sterngate mod apply mods/w211_top_speed_300.sgmod --vin WDB2112061A000001

# 3. Create a new community mod package
sterngate mod create --name "EGR Airmass Offset" --author "TunerKim" \
  --description "Increases fresh air mass by 40mg to minimize intake manifold soot" \
  --chassis W211 --ecu EDC16 --did 0x0115 --data "0028" \
  --output mods/egr_offset.sgmod --armor

# 4. List all installed mod packages in local library
sterngate mod library
```

