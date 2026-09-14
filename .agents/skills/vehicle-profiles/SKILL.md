---
name: vehicle-profiles
description: >-
  Instructions for authoring, validating, and converting vehicle profiles (JSON/TOML)
  for Sterngate, including extraction from CBF, SMR-D, and ODX databases.
---

# Vehicle Profiles Authoring & Management

Use this skill when adding support for a new vehicle, engine, or transmission in Sterngate.

## 1. Profile Structure

Profiles reside in `profiles/<oem>/<model>_<engine>.json`. They define how Sterngate maps human-readable names to diagnostic identifiers without hardcoding ECU addresses.

### Minimal Example
```json
{
  "profile_name": "Mercedes_W211_OM646_EDC16",
  "oem": "Mercedes-Benz",
  "chassis": "W211/S211",
  "gateway_type": "CGW_N93",
  "default_bitrate": 500000,
  "modules": {
    "EDC16": {
      "name": "Engine Control Unit (Bosch EDC16C31/CP31)",
      "names": {
        "en": "Engine Control Unit (Bosch EDC16C31/CP31)",
        "de": "Motorsteuergerät (Bosch EDC16C31/CP31)",
        "sv": "Motorstyrenhet (Bosch EDC16C31/CP31)"
      },
      "tx_id": "0x7E0",
      "rx_id": "0x7E8",
      "protocol": "UDS",
      "seed_key_algo": "Daimler_Level1"
    },
    "EGS52": {
      "name": "Electronic Transmission Control (722.6)",
      "names": {
        "en": "Electronic Transmission Control (722.6)",
        "de": "Elektronische Getriebesteuerung (722.6)",
        "sv": "Elektronisk transmissionsstyrning (722.6)"
      },
      "tx_id": "0x7E1",
      "rx_id": "0x7E9",
      "protocol": "KWP2000",
      "seed_key_algo": "Daimler_Level1"
    }
  },
  "parameters": [
    {
      "id": "trans_oil_temp",
      "name": "Transmission Fluid Temperature",
      "names": {
        "en": "Transmission Fluid Temperature",
        "de": "Getriebeöltemperatur",
        "sv": "Transmissionsoljetemperatur"
      },
      "module": "EGS52",
      "service": 34,
      "did": "0x2001",
      "byte_offset": 3,
      "length": 1,
      "scaling": {
        "slope": 1.0,
        "offset": -40.0
      },
      "unit": "°C",
      "min": -40.0,
      "max": 150.0
    }
  ]
}
```

---

## 2. Ingesting Legacy Formats (CBF / ODX / SMR-D) to Native Sterngate JSON

Sterngate is completely independent of proprietary binary formats. Legacy OEM files (such as Daimler Caesar Binary Format `.CBF`, ODX, or SMR-D) on diagnostic dumps or network shares are strictly **one-way input sources**. 

Once extracted into native Sterngate JSON, the legacy binary files are discarded and are **never committed or required at runtime**.

### One-Way Extraction Workflow
1. Extract CBF files from your diagnostic archive:
   ```bash
   python3 -c '
   import py7zr
   with py7zr.SevenZipFile("/path/to/diagnostic_archive.7z", "r") as z:
       z.extract(path="/tmp/cbf_extracted", targets=["Old_211_219/cbf/CR4.CBF", "Old_211_219/cbf/EGS52.CBF"])
   '
   ```
2. Generate the native Sterngate JSON profile:
   ```bash
   python3 scripts/cbf_extractor.py \
     --cbf /tmp/cbf_extracted/Old_211_219/cbf/CR4.CBF \
           /tmp/cbf_extracted/&_204_207_212_218/cbf/VGSNAG2.CBF \
           /tmp/cbf_extracted/Old_211_219/cbf/SBC211.CBF \
           /tmp/cbf_extracted/Old_211_219/cbf/ZGW211.CBF \
     --profile-id mercedes_w211_om642_cr4 \
     --oem Mercedes-Benz \
     --chassis W211/S211 \
     --output profiles/mercedes/w211_om642_cr4.json
   ```

### Automated Ingestion via Native CLI (`sterngate profile import`)
Sterngate features an automated CBF & SMR-D batch importer built into the binary:
```bash
# Ingest an entire extracted CBF/SMR-D folder or single archive
sterngate profile import --input /path/to/extracted_cbf --output profiles/

# Ingest single CBF file
sterngate profile import --input /path/to/CR4.CBF --output profiles/
```
The importer extracts all metadata, maps CAN arbitration IDs from the 990-ECU database, extracts DIDs, and generates production-ready JSON profiles.

---

---

## 3. Automotive ECU Catalog & Modular Schemas

Sterngate provides two formal JSON schemas for diagnostic definition:
1. **Full Vehicle Pack Schema** (`profiles/schema/sterngate-profile.schema.json`): Encapsulates a complete multi-ECU chassis architecture (gateway, modules, parameters, scaling, and seed-key algorithms).
2. **Modular ECU Definition Schema** (`profiles/schema/sterngate-ecu.schema.json`): Encapsulates an individual ECU module (e.g. `EGS52`, `EDC16`, `VGSNAG2`) with its diagnostic routing, physical & functional CAN arbitration IDs, supported DIDs, UDS Service 0x31 routines, variant coding layouts, and localized DTC definitions.

### Compact Runtime Index (`data/ecu_catalog.json`)
The canonical database contains **1,347 unique automotive ECUs** in a 21st-century compact format (1 line per ECU, JSON routing index) covering architectures from 1996 through 2024:
```json
{"ecu_name": "EGS52", "protocol": "UDS", "tx_id": "0x7e1", "rx_id": "0x7e9", "func_id": "0x7df", "dtc_count": 114, "chassis": ["ML_W163", "SLK_R170", "SLK_R171", "C_Class_W202", "W203/C209", "W211/C219", "S_W220/CL_W215", "W221/C216", "SL_R230"]}
{"ecu_name": "ESP223", "protocol": "UDS", "tx_id": "0x7e0", "rx_id": "0x7e8", "func_id": "0x7df", "dtc_count": 984, "chassis": ["W223"]}
```

### Multilingual Diagnostic Trouble Code (DTC) Database (`data/dtc_database_mb.json`)
Indexed directly from factory diagnostic simulations, containing **17,857 DTC entries** with dual English and German fault texts:
- Standard 5-character OBD/ISO codes (`P0100`, `P0560`, `U0100`, `C1500`)
- Factory 7-character UDS codes with Failure Type Bytes (`P164456`, `P056000`, `U010087`)

### CLI Catalog Commands
```bash
# Display overall ECU database statistics
sterngate ecu stats

# Search ECUs by name or chassis keyword
sterngate ecu search EGS
sterngate ecu search W223

# Detailed inspection of an ECU (protocol, CAN IDs, DTC count, chassis list)
sterngate ecu inspect MED1775
sterngate ecu inspect ESP223
```

---

## 4. CLI Profile Management

Sterngate provides direct CLI commands to inspect and list installed vehicle packs:

```bash
# List all discovered profiles with module counts and parameter stats
sterngate profile list

# Inspect detailed ECU routing and DID scaling
sterngate profile inspect profiles/mercedes/w211_om642_cr4.json
```

---

## 5. Parameter Scaling Types

Sterngate supports:
- **Linear**: `slope * raw + offset`
- **Bitmask**: Extract specific bit flags (e.g. lockup clutch status `0x00 = Open`, `0x01 = Slipping`, `0x02 = Closed`)
- **Signed 16-bit**: Two's complement for negative values (e.g. injector quantity deviation $-5.0\text{ to }+5.0\text{ mm}^3$)

