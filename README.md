# Sterngate (`sterngate`)

[![Rust](https://img.shields.io/badge/rust-1.89%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![MCP](https://img.shields.io/badge/MCP-2024--11--05-brightgreen.svg)](https://modelcontextprotocol.io)

**Sterngate** is a high-performance, modular automotive diagnostic, live telemetry, variant coding, and safe ECU flashing platform written in modern Rust. It replaces legacy, proprietary OEM diagnostic tooling (such as Mercedes Vediamo and Star / Xentry) with an open-source, cross-platform architecture that eliminates Windows XP virtual machines, license dongles, and fragile COM port setups.

---

## Key Capabilities

- **Beyond Generic OBD-II**: Interrogates manufacturer-specific DIDs through the vehicle's Central Gateway (CGW) using UDS (ISO 14229) and KWP2000 (ISO 14230). Read 722.6 automatic transmission fluid temperatures (for the crucial 80°C level check), cylinder-by-cylinder smooth running injector balances, torque converter clutch slip, and Airmatic line pressures.
- **Universal Modularity**: Decouples vehicle profiles from executable code. Profiles are stored in declarative JSON schemas under `profiles/`. Switch between a Mercedes W211 OM646 CDI, a VAG Golf Mk6 2.0 TDI (EDC17 + DSG), or a BMW E90 3.0d (DDE6) without recompiling the binary.
- **Decoupled Safe Flashing**: Atomic local file staging, strict pre-flight safety gates (voltage $\ge 12.5\text{ V}$, SHA256 & Bosch CRC32 verification, HW/SW calibration match), and a detached Tokio worker immune to browser closes or network drops.
- **P2P Remote Operations (Iroh)**: Integrated peer-to-peer QUIC tunneling allows an end customer to plug the device into their OBD port and share a short Node Ticket with a remote technician anywhere in the world—punching through carrier-grade NATs without port forwarding.
- **Built-in Model Context Protocol (MCP) Server**: Exposes diagnostic routines, live telemetry snapshots, fault code scanning, and flash safety verification directly to AI agents.
- **Hardware Abstraction Layer (HAL)**: Native support for Linux SocketCAN (`can0`, CANable, gs_usb, Candlelight, SPI MCP2518FD), SAE J2534 PassThru (Tactrix OpenPort 2.0), and zero-hardware virtual simulation.

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

#### A. Local Standalone Mode (In-Car Raspberry Pi / SBC)
```bash
sterngate --local --can-interface can0 --port 8080
```

#### B. Customer Car-Bridge Mode (P2P Listener)
```bash
sterngate --client --can-interface can0
```
Prints an encrypted Iroh Node Ticket to share with the remote technician.

#### C. Remote Technician Mode (Dialing the Car)
```bash
sterngate --server --ticket <NODE_TICKET> --port 3000
```
Connects over end-to-end encrypted QUIC directly into the car's gateway and serves the diagnostic dashboard on localhost:3000.

#### D. Model Context Protocol (MCP) Server for AI Agents
```bash
sterngate mcp
```
Communicates over standard input/output (JSON-RPC 2.0).

---

## OBD-II Port Wiring Reference

For Mercedes W211/S211 and most ISO 15765-4 compliant vehicles:

| OBD-II Pin | Signal Description | Hardware Connection |
| :--- | :--- | :--- |
| **Pin 4** | Chassis Ground | Ground (GND) |
| **Pin 5** | Signal Ground | Ground (GND) |
| **Pin 6** | CAN High (CAN-D Diagnostics) | CAN_H (500 kbps) |
| **Pin 14** | CAN Low (CAN-D Diagnostics) | CAN_L (500 kbps) |
| **Pin 16** | Battery Power (+12V Continuous) | Power Input / Voltage Sensing |

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.

