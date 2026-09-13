---
name: sterngate-ops
description: >-
  Operational runbook for running, testing, and troubleshooting Sterngate in various modes:
  local SBC dashboard, client car-side P2P bridge, remote technician server, and virtual simulation.
---

# Sterngate Operations Runbook

Use this skill when building, starting, or verifying Sterngate in any of its operational modes.

## 1. Quick Mode Selection

| Mode | Command | Use Case | Requirements |
| :--- | :--- | :--- | :--- |
| **Local** | `sterngate --local` | In-car standalone SBC (Raspberry Pi / laptop) serving Web UI on Wi-Fi | Local CAN adapter (`can0`) |
| **Client** | `sterngate --client` | Customer car-side node bridging CAN to P2P Iroh tunnel | Local CAN adapter (`can0`) + internet |
| **Server** | `sterngate --server --ticket <TICKET>` | Remote technician laptop dialing customer node | Internet access |
| **Mock** | `sterngate mock` | Offline development and CI/CD testing with simulated W211 | None (zero hardware needed) |
| **MCP** | `sterngate mcp` | Running as a Model Context Protocol tool provider for AI agents | stdio |

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

---

## 3. Running Standalone Mock Simulation

If you do not have root access or kernel vcan support, launch the built-in mock simulator:
```bash
cargo run --package sterngate-cli -- mock --port 8080
```
This spawns the internal `VirtualCanInterface`, simulating a Mercedes-Benz W211 with live OM646 engine and 722.6 transmission telemetry. You can open `http://localhost:8080` in your browser to inspect live gauges, read DTCs, and test variant coding.

---

## 4. P2P Remote Diagnostics Flow

### Step A: Car-Side Customer
```bash
sterngate --client --can-interface can0
```
Output:
```
=============================================================
  Sterngate Car-Side Node Active
  Node Ticket: node:abc123456...
=============================================================
Share this ticket with your remote technician.
```

### Step B: Remote Technician
```bash
sterngate --server --ticket node:abc123456... --port 3000
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

## 11. Verification Checklist

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
   ```



