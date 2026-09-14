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
Mod-ID: amg_needle_sweep_20260914
Name: AMG Needle Sweep
Author: CommunityTuner
Target-Chassis: W211
Target-ECU: IC_211 (0x7E0)
CRC32: 5831E09F
SHA256: 6e7b5bf2fede5951d756e44ee4fa6e3f677757bae4e5d4ac685c31e8f1b65e95
FEC: ReedSolomon_GF256

<base64-encoded-payload-with-fec-parity-blocks>
-----END STERNGATE COMMUNITY MOD-----
```

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
Applies the mod safely with live voltage checks and Git garage history:
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
- `sterngate_apply_community_mod`: Safely executes the mod on the vehicle or simulated ECU with voltage interlock and Git garage logging.
- `sterngate_create_community_mod`: Authors a compliant `.sgmod` package and outputs ASCII armor.
- `sterngate_scan_rom_maps`: Scans a ROM binary for calibration maps, Bosch IDs, and checksum blocks via `rom_path` or `rom_base64`.
- `sterngate_generate_stage_tune`: Creates Stage 1 or Stage 2 `.sgmod` tuning packages directly from ROM dumps.
- `sterngate_kill_dtc`: Generates a standalone `.sgmod` suppressing specific DTC error codes.
- `sterngate_solve_checksum`: Verifies and optionally recalculates Bosch MPC5xx partitioned checksums.

