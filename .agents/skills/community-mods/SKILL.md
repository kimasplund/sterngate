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

## 4. Web UI Dashboard

On the embedded Sterngate Dashboard (`http://localhost:8080`), open **Tab 4 (Diagnostics & Coding)**:
1. **Dropzone / Paste**: Drag and drop any `.sgmod` file, or paste ASCII armored text into the text box.
2. **Live Inspection**: Click **Inspect Mod** to view author metadata, target vehicle filters, bitmasks, and Reed-Solomon auto-repair diagnostics.
3. **1-Click Apply**: Click **Apply Mod to Vehicle** to execute over CAN bus with live progress bar and Git commit SHA display.
4. **Mod Creator Wizard**: Click **Create New Community Mod** to open the authoring modal, specify parameters, and instantly download the `.sgmod` package or copy armored text.

---

## 5. Model Context Protocol (MCP) Tools

AI assistants can interact with community mods using standard MCP tools:
- `sterngate_inspect_community_mod`: Cryptographically validates payload, verifies chassis compatibility, and tests Reed-Solomon error correction.
- `sterngate_apply_community_mod`: Safely executes the mod on the vehicle or simulated ECU with voltage interlock and Git garage logging.
- `sterngate_create_community_mod`: Authors a compliant `.sgmod` package and outputs ASCII armor.
