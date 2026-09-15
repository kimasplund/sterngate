---
name: community-mods
description: Complete authoring, validation, error-correction, and deployment guide for Sterngate shareable community mod packages (.sgmod and ASCII armored text).
---

# Sterngate Community Mods & Tuning Packages Runbook

Sterngate community mods (`.sgmod`) provide a safe, fault-tolerant, shareable format for automotive parameter tuning, variant coding retrofits, and ECU adaptations.

---

## 1. Core Architecture & Guarantees

### Forward Error Correction (Reed-Solomon $GF(2^8)$)
When mods are shared across automotive forums, Discord channels, pastebins, or SMS, formatting quirks, character dropouts, or bit corruption frequently break checksums.
- **FEC Scheme**: Reed-Solomon $(255, 239)$ over Galois Field $GF(2^8)$ with irreducible generator polynomial $p(x) = x^8 + x^4 + x^3 + x^2 + 1$ ($0x11D$).
- **Correction Capability**: Automatically repairs up to **8 corrupted bytes per block** ($t = 8$, 16 parity bytes per 239-byte payload block) using Berlekamp-Massey error locator, Chien search, and Forney evaluation.
- **Zero-Roundtrip Self-Healing**: A user can copy corrupted or mangled armored text, and Sterngate will automatically reconstruct the missing or flipped bytes without failing or requiring re-download.

### Cryptographic Verification
- **Dual Checksum**: Every package carries both a fast CRC32 checksum and a cryptographic SHA-256 hash.
- **Precondition & Bitmask Safety**: Only target bits specified in the bitmask are altered; live vehicle configuration bits outside the mask are preserved intact.

### Strict Target Fingerprinting
Mods will **never** execute on an incompatible vehicle or ECU:
- `chassis`: Whitelist of chassis codes (e.g. `["W211", "S211", "C219"]`), verified against the vehicle VIN.
- `ecu_name`: ECU module name (e.g. `IC_211`, `EDC16`, `EGS52`).
- `compatible_hw_ids`: Optional whitelist of OEM/Bosch hardware part numbers (e.g. `0281013854`).
- `min_battery_voltage`: Refuses to execute if battery voltage is below threshold (e.g. $12.0\text{ V}$).
- `requires_engine_off`: Refuses to execute if the engine is running.

### Atomic Git Garage Rollback
Before applying any community mod, Sterngate creates a pre-mod baseline snapshot in the local Git-backed Vehicle Garage. Applying the mod creates a traced Git commit. Reverting to stock is an atomic 1-click or 1-command git checkout operation.

---

## 2. ASCII Armored Block Format

A community mod can be saved as a `.sgmod` JSON file or shared directly as human-readable armored text:

```text
-----BEGIN STERNGATE COMMUNITY MOD-----
Mod-ID: amg_needle_sweep_20260915
Name: AMG Needle Sweep
Author: CommunityTuner
Target-Chassis: W211
Target-ECU: IC_211 (0x7E0)
CRC32: 995DA3D8
SHA256: 235c53cf7472c05531a47a9d122e2c24c1d234ebd63cf7b0ab27703eb55e2567
FEC: ReedSolomon_GF256

<base64-encoded-payload-with-fec-parity-blocks>
-----END STERNGATE COMMUNITY MOD-----
```

This sample is regenerated at `integrity.version` 2 (`profiles/mods/amg_needle_sweep.sgmod`); packages at version 1 are refused everywhere.

Sterngate automatically trims forum whitespace, handles Markdown backticks, and processes DOS/Unix line endings.

---

## 3. CLI Operational Workflows

### Inspect & Validate Mod
Inspect an `.sgmod` file, stdin, or raw armored text, testing chassis compatibility and self-healing:
```bash
# From file
sterngate mod inspect profiles/mods/amg_needle_sweep.sgmod --vin WDB2112061A123456

# From stdin / pipe
cat mod.txt | sterngate mod inspect -
```

### Apply Mod to Vehicle
Applies the mod safely with live voltage checks and Git garage history. `--vin` is required — applying without the connected vehicle's VIN is refused:
```bash
sterngate mod apply profiles/mods/amg_needle_sweep.sgmod --vin WDB2112061A123456
```

### Create a Mod Package
Author a new shareable package:
```bash
sterngate mod create \
  --name "AMG Instrument Needle Sweep" \
  --author "BenzTuner" \
  --description "Enables needle sweep on ignition ON" \
  --chassis "W211" \
  --ecu "IC_211" \
  --did "0x01B0" \
  --data "02" \
  --mask "02" \
  --category "comfort" \
  --risk "low" \
  --min-voltage 12.2 \
  --out "profiles/mods/amg_needle_sweep.sgmod"
```

### List Installed Community Mods
```bash
sterngate mod list --dir profiles/mods
```

---

## 4. Flash Calibration Tuning & WinOLS Map Studio

Sterngate extends community mods beyond diagnostic variant coding into **ECU flash calibration tuning**, eliminating risky 2MB full ROM flashing in favor of compact, verified, self-healing `.sgmod` diff patches.

### Supported Mod Actions
- `ModAction::ConfigureDid`: UDS Service 0x2E/0x2E with bitmask and precondition validation.
- `ModAction::PatchFlashMap`: In-place calibration map write (Service 0x3D / `WriteMemoryByAddress`) to ECU calibration sector (`0x1C0000..0x1FFFFF` on EDC16) with address offset, expected stock data, and safety ceiling clamping.
- `ModAction::DtcMask`: Zeroes DTC fault path enable switches in the calibration sector to suppress specific error codes (e.g. `P0401` EGR, `P2002` DPF).

### CLI Tuning Suite (`sterngate tune`)
```bash
# Heuristic map scan, Bosch HW/SW detection, and MPC5xx checksum verification
sterngate tune scan rom.bin

# Generate a Stage 1 tuning .sgmod (+18% Torque, +120 mbar Boost, +50 bar Rail)
sterngate tune stage1 rom.bin --chassis "W211" --ecu "EDC16CP31" --out stage1.sgmod

# Generate a Stage 2 tuning .sgmod (+25% Torque, +200 mbar Boost, DPF/EGR Delete, DTC Kill)
sterngate tune stage2 rom.bin --chassis "W211" --ecu "EDC16CP31" --out stage2.sgmod

# Generate a standalone DTC suppression .sgmod
sterngate tune dtc-kill rom.bin --codes "P0401,P2002" --out dtc_kill.sgmod

# Verify or fix Bosch MPC5xx 32-bit block checksums and complement pairs
sterngate tune checksum rom.bin --fix --out rom_fixed.bin
```

---

## 5. Web UI Map Studio & Tuning Tab

On the embedded Sterngate Dashboard (`http://localhost:8080`), open **Tab 9 (Map Studio & Tuning)**:
1. **ROM Binary Loader**: Drag & drop any 2MB Bosch EDC16 binary dump or click **Load Sample EDC16 ROM** for instant testing.
2. **Identification & Integrity**: Instant display of Bosch Hardware (`0281...`), Software (`1037...`), and 4 MPC5xx partitioned block checksums.
3. **1-Click Stage Generator**: One-click creation of Stage 1, Stage 2, or custom DTC suppression packages with live download and direct vehicle dispatch.
4. **Interactive 2D/3D Map Selector**: Inspect Driver's Wish, Torque Limiter, Boost Target, SVBL, Smoke Limiter, Rail Pressure, and EGR Hysteresis.
5. **Dynamic Heatmapped Table**: Visual HSV color gradients (green $\to$ yellow $\to$ red) reflecting cell intensity with percentage modification inputs and safety clamping.
6. **In-Place Checksum Recalculation**: Fix invalid block checksums with 1 click and download corrected ROM binaries.

---

## 6. Model Context Protocol (MCP) Tools

AI assistants can interact with community mods and calibration tuning using standard JSON-RPC MCP tools:
- `sterngate_inspect_community_mod`: Cryptographically validates payload, verifies chassis compatibility, and tests Reed-Solomon error correction.
- `sterngate_apply_community_mod`: **Simulated only.** The MCP server hands every tool a `VirtualCanInterface`, so this executes the mod against the built-in virtual ECU and never against a real vehicle. The gates (provenance, integrity, voltage interlock, byte preconditions) and Git garage logging all run, and the response carries `"simulated": true`. Apply on real hardware with `sterngate mod apply` or the REST API.
- `sterngate_create_community_mod`: Authors a compliant `.sgmod` package and outputs ASCII armor.
- `sterngate_scan_rom_maps`: Scans a ROM binary for calibration maps, Bosch IDs, and checksum blocks via `rom_path` or `rom_base64`.
- `sterngate_generate_stage_tune`: Creates Stage 1 or Stage 2 `.sgmod` tuning packages directly from ROM dumps. Phase 0: refuses on every ROM (`PreFlightCheckFailed`) until the detector rebuild.
- `sterngate_kill_dtc`: Generates a standalone `.sgmod` suppressing specific DTC error codes. Phase 0: refuses on every ROM (`PreFlightCheckFailed`) until the detector rebuild.
- `sterngate_solve_checksum`: Verifies and optionally recalculates Bosch MPC5xx partitioned checksums.

---

## 7. Safety contract (Phase 0)

- `PatchFlashMap` and `DtcMask` actions carry `"provenance"`; only `"scanned"` (bytes located in the target ECU's own ROM) is executable. Absent or any other value is refused before any bus traffic.
- Every flash patch must carry `expected_original_data` of exactly the patched length; the runner reads the live bytes and refuses on read failure, short reply or mismatch. `DtcMask` requires the live byte to equal `original_mask`.
- Packages that write flash require `min_battery_voltage >= 12.5`; `SterngateMod::create` refuses lower values and `ModRunner` enforces `max(min_battery_voltage, 12.5)` regardless of the CLI `--force` flag.
- `--force` (CLI only) relaxes the chassis and hardware-whitelist checks and nothing else. It is refused for packages containing flash writes. The REST API and MCP have no bypass: sending `force` returns HTTP 422 / an MCP error.
- `integrity.version` is 2 and covers `target`, `actions` and `rollback_actions`. Packages with any other version are refused everywhere (inspect included); regenerate them with `sterngate mod create`.
- Applying requires the connected vehicle's VIN (`--vin`, `vin`) and a measured battery voltage. The CLI reads the Tactrix OpenPort Pin-16 ADC and refuses on SocketCAN or mock adapters, which cannot measure.
- `sterngate tune stage1|stage2|dtc-kill` refuse on every ROM until the detector rebuild locates real maps; this is intended.
- `Routine` actions with routine id `0xFF00` (UDS EraseMemory) are refused; flash erase only happens through the flashing worker's interlocks.
- Inspect (CLI, REST, MCP) reports the same refusals as apply: provenance, EraseMemory, the 12.5 V floor. Both paths share `SterngateMod::flash_write_refusals` (apply's gate) and `SterngateMod::check_compatibility` (inspect's report), so nothing that would be refused at apply time is silently passed by inspect.
- `mod apply` keeps the extended session alive with a suppressed TesterPresent (`S3KeepAlive`) before every read and write, so however slow the ECU is, no gap between the last precondition read and the first write goes longer than 1500 ms without a `3E 80`. The blocking pre-mod garage commit is taken **before** `10 03` is entered — it needs nothing from the ECU — so git and disk work never sits inside the session at all; the post-mod commit runs after the last bus exchange, where an expired S3 timer no longer matters.

