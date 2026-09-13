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

## 8. Verification Checklist

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
   curl -s http://localhost:8080/api/v1/recorder/status | jq .
   curl -s http://localhost:8080/api/v1/ecu/stats | jq .
   curl -s "http://localhost:8080/api/v1/ecu/search?q=EGS52" | jq .
   ```



