# Sterngate: Agent Guidelines and System Architecture Handbook

Welcome to the **Sterngate** codebase. This document serves as the primary context, architectural blueprint, and operational instruction manual for autonomous agents and developers working on this project.

---

## 1. Project Mission & Identity

**Sterngate** is a high-performance, modular automotive diagnostics, telemetry, variant coding, and safe ECU flashing platform written in modern Rust. It replaces legacy, proprietary OEM software (e.g., Mercedes Vediamo and Xentry) and transcends generic OBD-II scanners.

### Core Distinctions
- **Beyond Generic OBD-II**: Reads manufacturer-specific DIDs (e.g., 722.6 transmission fluid temperature, smooth-running injector balances, torque converter lockup slip) by routing KWP2000 / UDS requests through the vehicle gateway.
- **Universal Modularity**: Vehicle specifics (arbitration IDs, DIDs, scaling equations, seed-key algorithms) are stored in declarative JSON/TOML profiles under `profiles/`. Adding support for a new vehicle (e.g., VAG EDC17, BMW DDE6) requires zero binary recompilation.
- **Decoupled Safe Flashing**: All firmware binaries are staged locally and verified (hardware ID, software calibration, CRC32/SHA256) prior to execution. The flashing worker runs detached in real-time, locking out conflicting APIs (HTTP 423) and maintaining strict ISO-TP / TesterPresent heartbeats regardless of browser or network dropouts.
- **P2P Remote Operations**: Powered by **Iroh** QUIC tunneling. Technicians can diagnose, code, and stage flashes remotely across carrier-grade NATs without static IPs or port forwarding using simple node tickets.
- **Native MCP Server**: Exposes diagnostic inspection, DTC reading/clearing, telemetry streams, and flashing safety gates directly to AI agents via standard Model Context Protocol over stdio.

---

## 2. Safety Critical Invariants & Rules

When modifying or generating code for Sterngate, you **MUST** strictly adhere to the following safety invariants:

> [!CAUTION]
> **ECU Bricking Prevention**: Automotive ECUs (such as Bosch EDC16C31/CP31) entering a bootloader state will become inoperable ("bricked") if an erase or write routine is interrupted or corrupted.
>
> 1. **Local Staging Only**: Never stream firmware over network or WebSocket during flash writes. Firmware must be fully staged and cryptographically verified on local disk first.
> 2. **Voltage Interlock**: Refuse to trigger erase (`0x31 Routine 0xFF00`) unless the measured system voltage is $\ge 12.5\text{ V}$.
> 3. **Keep-Alive Guarantee**: Maintain the $S_3$ session timer via `TesterPresent` (`0x3E 80`) every $2000\text{ ms}$ whenever an extended or programming session (`0x10 02` / `0x10 03`) is active.
> 4. **API Lockout**: When the Flashing State Machine transitions out of `Idle`, all diagnostic reads, variant coding, and external API requests must immediately yield `HTTP 423 Locked`.
> 5. **Zero-Trust Command Integrity**: Any mutating request (variant coding, routine actuation, flash trigger) arriving from an external interface (Web UI, REST API, or P2P QUIC link) must be encapsulated in a `CommandEnvelope`. The core `TransactionGate` MUST verify exact payload length, CRC32 checksum, timestamp freshness (TTL), and idempotency key before dispatching any bytes to the CAN bus. Unverified or truncated commands are dropped immediately.
> 6. **Error Handling**: Never panic (`unwrap()`) in protocol state machines or CAN dispatch loops. Use strongly typed `SterngateError` with graceful recovery.

---

## 3. Workspace Architecture

Sterngate is organized as a Cargo workspace with distinct layers:

```
crates/
├── sterngate-core/        # Core domain types (CanFrame, Parameter, Dtc, Profile, FlashState)
├── sterngate-hal/         # Hardware abstraction (SocketCAN, Virtual/Mock CAN, J2534, DoIP)
├── sterngate-protocol/    # ISO-TP (ISO 15765-2), UDS (ISO 14229), KWP2000, Seed-Key Solvers
├── sterngate-p2p/         # Iroh QUIC tunneling, NodeTicket generation, RPC & Blob staging
├── sterngate-server/      # Axum REST API, WebSocket 50Hz telemetry, embedded UI dashboard
├── sterngate-mcp/         # Native Model Context Protocol (MCP) server for AI assistants
└── sterngate-cli/         # Main binary executable (`sterngate`) with subcommands
```

### Module Responsibilities

1. **`sterngate-core`**:
   - Houses domain models with zero external heavy dependencies.
   - `CanFrame`: 11-bit standard and 29-bit extended frames with microsecond timestamps.
   - `VehicleProfile`: JSON-deserializable profile describing ECU addresses, DIDs, scaling formulas, and units.
   - `FlashPackage`: Manifest structure for staging firmware with target hardware and checksums.
   - `CascadeWatchdog`: Autonomous detection engine for 13 notorious Mercedes-Benz cascades of death.
   - `CompressorProtectionGuard` & `SuspensionLeakDetector`: Thermal watchdog (40s auto-cutoff) and pneumatic leak diagnostics.
   - `VehicleGarage` & `DecodedVin`: Local Git-backed per-vehicle configuration tracking and VIN decoder.
   - `DriveBenchmark`: High-frequency drive telemetry sampling and A/B comparative benchmark analysis.

2. **`sterngate-hal`**:
   - `VehicleInterface`: The unified asynchronous trait (`send`, `recv`, `open`, `close`, `set_filter`).
   - `SocketCanInterface`: Native Linux CAN interface (`can0`, `can1`, `vcan0`).
   - `VirtualCanInterface`: In-memory emulator that simulates Mercedes W211 Central Gateway (N93), EDC16 engine, EGS52 transmission, and ABC/ENR suspension for offline tests.
   - `J2534Interface`: PassThru API bridge for hardware like Tactrix OpenPort 2.0.

3. **`sterngate-protocol`**:
   - `IsoTpChannel`: Asynchronous ISO 15765-2 layer handling Single Frame, First Frame, Consecutive Frame, and Flow Control (`0x30`).
   - `UdsClient`: Standard ISO 14229 diagnostics client.
   - `SeedKeyRegistry`: Algorithmic solvers (Daimler standard Level 01, Level 03, Level 0B) without proprietary Windows DLLs.
   - `FlashingWorker`: Decoupled Tokio state machine managing erase, download, transfer, exit, and CRC routines.
   - `VehicleScanner`: Multi-ECU gateway scanner, ENR compressor control, and ABC hydraulic surge limiter.
   - `BusDiscoverer`: CAN ID range probing (`0x700..0x7EF`), identification DID interrogation, and auto profile generation.
   - `ServiceRoutineManager`: Safety-critical workshop service routines (SBC pad mode 0 bar deactivation/reactivation, Common Rail IMA coding, suspension corner actuation).

4. **`sterngate-p2p`**:
   - Wraps Iroh for P2P QUIC communication between `--bridge` (car-side SBC host) and `--tech` (remote technician client).
   - Generates and dials `NodeTicket` strings.

5. **`sterngate-server`**:
   - Exposes REST routes (`/api/v1/telemetry`, `/api/v1/dtc`, `/api/v1/coding`, `/api/v1/flash`, `/api/v1/vehicles`, `/api/v1/analyze/cascades`, `/api/v1/abc/control`, `/api/v1/service/sbc`, `/api/v1/service/ima`, `/api/v1/service/suspension`, `/api/v1/diag/discover`, `/api/v1/diag/report.html`).
   - Streams live parameters over WebSockets at 20–50 Hz.
   - Serves the embedded single-page dashboard at `http://localhost:8080` localized in English, German, and Swedish.

6. **`sterngate-mcp`**:
   - Implements JSON-RPC 2.0 stdio Model Context Protocol.
   - Enables AI agents to read DTCs, inspect live telemetry, check vehicle profiles, evaluate 13 cascades of death, actuate compressor/ABC safety guards, run pre-flash checks, discover uncataloged ECUs, dispatch workshop service routines, execute detached flashing, and export HTML diagnostic reports.

7. **`sterngate-cli`**:
   - Clap CLI interface unifying all operational modes:
     * `sterngate --local`: Standalone SBC + local UI.
     * `sterngate --bridge` (alias `--car`): In-car diagnostic bridge + Iroh P2P host.
     * `sterngate --tech --ticket <TICKET>` (alias `--ticket`): Remote technician diagnostic client.
     * `sterngate mcp`: Model Context Protocol server.
     * `sterngate mock`: Virtual simulation mode for zero-hardware testing.
     * `sterngate diag <subcommand>`: Direct CLI diagnostic utilities (dtc, live, clear, routine, scan, discover).
     * `sterngate flash <subcommand>`: Direct terminal ECU flashing suite (stage, preflight, start, status).
     * `sterngate service <subcommand>`: Workshop service routines (sbc, ima, suspension).
     * `sterngate coding <subcommand>`: Variant coding & Git garage history (read, write, backup, diff).
     * `sterngate ecu <subcommand>`: 990-ECU diagnostic catalog index (stats, search, inspect).
     * `sterngate profile <subcommand>`: Vehicle profile management (list, inspect, generate).
     * `sterngate analyze <subcommand>`: Predictive analytics and containment (suspension, compare, cascades, abc).

---

## 4. Progressive Skills Suite

Specialized agent skills are maintained under `.agents/skills/`:
- **`sterngate-ops`**: Operational runbook for launching and troubleshooting all CLI modes.
- **`sterngate-mcp`**: Integration guide for calling Sterngate MCP tools and resources.
- **`vehicle-profiles`**: Guide for converting CBF/ODX files and defining JSON vehicle profiles.
- **`safe-flashing`**: Pre-flight checklist, voltage interlocks, and recovery runbooks.

Whenever you add or modify protocols, CLI flags, or profile schemas, you **MUST** update the corresponding skill file.

---

## 5. Coding & Development Standards

- **Rust Edition & Toolchain**: Target modern Rust (Edition 2021/2024 features). Use `tokio` for async runtimes.
- **Linting & Formatting**:
  - Run `cargo fmt --check` before committing.
  - Run `cargo clippy --workspace --all-targets -- -D warnings`.
- **Testing**:
  - Always write unit tests for protocol frame parsers, scaling formulas, and state machines.
  - Use `VirtualCanInterface` in integration tests so tests pass deterministically on CI without real CAN hardware.
- **Documentation**: Provide clear Rust doc comments (`///`) on all public types, traits, and functions.
- **Commit on Save**: Whenever a coherent set of file modifications or feature enhancements is completed, immediately format, test, and commit the changes to git history (`git add . && git commit -m "..."`). Never leave uncommitted changes hanging between sessions.

