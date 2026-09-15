---
name: safe-flashing
description: >-
  Safety rules, pre-flight checklists, voltage interlocks, and recovery runbooks
  for ECU flashing and parameter programming in Sterngate.
---

# Safe ECU Flashing & Parameter Programming Runbook

> [!CAUTION]
> Flashing an ECU carries inherent risk of rendering the vehicle non-operational.
> You must strictly adhere to the pre-flight checks and safety protocols outlined here.

---

## 1. The Pre-Flight Verification Gate

Before initiating an ECU flash sequence (`0x34 RequestDownload` / `0x31 01 FF 00 Erase`):

1. **Battery Supply Voltage**:
   - Must be measured $\ge 12.5\text{ V}$ continuously for at least 10 seconds.
   - A dedicated automotive power supply / battery maintainer (e.g., GYSflash, Deutronic) providing at least 30A must be connected.
   - Refuse flash if voltage $< 12.5\text{ V}$.
2. **Local Binary Staging**:
   - The flash image must exist on the local filesystem (`/var/run/sterngate/staging.bin` or temp directory).
   - Checksum verification (SHA256 and Bosch CRC32) must be calculated locally and validated against the manifest.
3. **Hardware / Software Compatibility**:
   - Query ECU Hardware Version (`Service 0x22 DID 0xF191`).
   - Query ECU Calibration ID (`Service 0x22 DID 0xF190`).
   - Abort immediately if the staged image's target ID does not match the ECU hardware ID.

---

## 2. Flashing Execution State Machine

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> StageAndVerify: Load ROM & Manifest
    StageAndVerify --> PreFlightChecks: Local SHA256 & HW Match OK
    PreFlightChecks --> ApiLockout: Battery >= 12.5V
    ApiLockout --> ExtendedSession: Lock all external APIs (HTTP 423)
    ExtendedSession --> SecurityUnlock: 0x10 03 (Extended Diag)
    SecurityUnlock --> SilenceBus: 0x27 Seed-Key Solver
    SilenceBus --> ProgrammingSession: 0x28 (Disable Normal Msg) + 0x85 (Disable DTC)
    ProgrammingSession --> EraseMemory: 0x10 02
    EraseMemory --> TransferBlocks: 0x31 01 FF 00 (POINT OF NO RETURN)
    TransferBlocks --> ExitTransfer: 0x34 + 0x36 Chunk Loop
    ExitTransfer --> ChecksumRoutine: 0x37
    ChecksumRoutine --> EcuReset: 0x31 01 02 02 Verified
    EcuReset --> ReleaseLockout: 0x11 01 (Hard Reset)
    ReleaseLockout --> Completed: Restore Normal Comms
    Completed --> [*]
```

---

## 3. Session Keep-Alive ($S_3$ Guard)

The flashing worker runs as a detached high-priority Tokio task (`SCHED_FIFO` or `nice -20` on Linux).
Between data blocks, it continuously guarantees:
- TesterPresent (`0x3E 80`) dispatched every $2000\text{ ms}$.
- If any frame is dropped or an unrecoverable NACK (`0x7F`) is received before erasing, the sequence aborts cleanly.
- If an error occurs *after* erasing has completed, the worker enters `RecoveryMode` and attempts re-flashing the recovery image.

---

## 4. Zero-Trust Command Integrity & Network Drop Resilience

All mutating commands (variant coding, parameter writes, routine actuation, and flash triggers) are treated as untrusted and potentially incomplete:

1. **Envelope Verification**:
   - Every request from Web UI or P2P tunnels must arrive encapsulated in a `CommandEnvelope`.
   - The core verifies exact byte length (`payload_len`) and IEEE 802.3 CRC32 (`payload_crc32`) match the raw payload.
   - Any truncated packet or network glitch immediately triggers `SterngateError::CorruptedPayload` with zero CAN transmission.
2. **Replay & Stale Command Protection**:
   - The envelope carries `timestamp_ms` and a strictly bounded `ttl_ms` (typically 3000ms).
   - If a request is delayed over high-latency cellular or reconnecting WiFi, it is rejected with `SterngateError::ExpiredCommand`.
   - `idempotency_key` guarantees commands are never executed more than once.
3. **Deterministic Local Autonomy**:
   - Once a flashing or long-running routine is validated and started, the core worker executes detached locally.
   - Loss of WebSocket connection, browser closing, or P2P QUIC disconnect NEVER interrupts the $S_3$ keep-alive or in-flight transfer blocks.

---

## 5. CLI Flashing Suite Commands

Sterngate provides direct terminal commands with built-in safety interlocks:

```bash
# 1. Stage and cryptographically verify firmware ROM
sterngate flash stage --module EDC16 --file /path/to/stage1.bin \
  --hw-id 0281012234 --sw-id 1037372120 --start-address 0x00040000

# 2. Run pre-flight safety checks (voltage >= 12.5V, HW ID match, CRC32/SHA256)
sterngate flash preflight --manifest /var/run/sterngate/flash_manifest.json

# 3. Start detached flash sequence (requires interactive confirmation or --yes)
sterngate flash start --manifest /var/run/sterngate/flash_manifest.json --yes

# 4. Monitor live progress of detached flashing worker
sterngate flash status
```

---

## 6. Local Firmware Vault & Calibration Upgrade Scanner

Sterngate keeps all firmware binaries strictly decoupled from remote networks and Git repositories:
1. **Local Vault Scanning**:
   - `GET /api/v1/vault/scan?path=<PATH>&hw_id=<HW>&sw_id=<SW>` scans the local firmware vault for `.bin`, `.rom`, `.cff`, `.smr-f`, and `.fls` files. `<PATH>` is interpreted **inside the configured vault root** (blank scans the whole vault).
   - The vault root is resolved once, in precedence order: `--vault <DIR>`, then `STERNGATE_VAULT_ROOT`, then `./firmware_vault`. Both vault routes are confined to it, so `../`, absolute paths, and symlinks that escape the root are rejected with `400`.
   - Extracts Bosch HW/SW IDs, OEM part numbers, and SHA256 hashes directly from raw binary headers.
2. **Auto-Matched Upgrade Recommendation**:
   - If a discovered binary matches the connected vehicle ECU hardware ID (`DID 0xF192`) but contains a newer calibration version (`DID 0xF194`), Sterngate generates a verified upgrade recommendation.
3. **One-Click Staging**:
   - `POST /api/v1/vault/stage` takes `file_path` and a mandatory `measured_voltage`, verifies the safety interlocks, computes CRC32/SHA256, and stages the firmware for flashing without streaming bytes over the network.
   - `measured_voltage` must come from an actual hardware reading. Both staging routes (`/api/v1/flash/stage` and `/api/v1/vault/stage`) return `400` when it is absent: the server has no voltage sensor of its own, and substituting a plausible constant would make the $\ge 12.5\text{ V}$ interlock impossible to fail.
   - `sterngate mod apply` and `POST /api/v1/mods/apply` follow the same rule: the CLI reads the OpenPort Pin-16 ADC through `VehicleInterface::measure_battery_voltage` and refuses on adapters without a sensor; the REST route requires `battery_voltage` and the connected VIN.



