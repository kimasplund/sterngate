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

## 6. Verification Checklist

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
   curl -s http://localhost:8080/api/v1/telemetry | jq .
   curl -s http://localhost:8080/api/v1/dtc | jq .
   curl -s http://localhost:8080/api/v1/recorder/status | jq .
   ```

