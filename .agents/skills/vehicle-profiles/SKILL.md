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
      "tx_id": "0x7E0",
      "rx_id": "0x7E8",
      "protocol": "UDS",
      "seed_key_algo": "Daimler_Level1"
    },
    "EGS52": {
      "name": "Electronic Transmission Control (722.6)",
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

## 2. Reverse Engineering CBF Files to Sterngate JSON

Sterngate includes an automated binary CBF extractor (`scripts/cbf_extractor.py`) to parse Daimler Caesar Binary Format files directly from the NAS archive (`smb://kims-nas.local/public/DTS Projects/`):

### Extraction Workflow
1. Extract CBF files from the archive:
   ```bash
   python3 -c '
   import py7zr
   with py7zr.SevenZipFile("/run/user/1000/gvfs/smb-share:server=kims-nas.local,share=public/DTS Projects/DTS_Daimler Refresh_V2 #fuckacmeinc.7z", "r") as z:
       z.extract(path="/tmp/cbf_extracted", targets=["Old_211_219/cbf/CR4.CBF", "Old_211_219/cbf/EGS52.CBF"])
   '
   ```
2. Generate the Sterngate JSON profile:
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

---

## 3. CBF Deduplication, Cataloging & Inspection

Sterngate includes an automated deduplication analyzer (`scripts/cbf_dedup_analyzer.py`) that indexes all 2,055 Vediamo CBF files into a canonical catalog (`data/cbf_catalog.json`).

### Catalog Statistics
- **Total CBF files**: 2,055
- **Unique ECUs**: 990
- **Duplicate groups**: 412 (846 files or 41.2% are exact byte-for-byte duplicates across chassis folders)
- **Multi-version ECUs**: 172 ECUs have multiple chronological revisions (e.g., `VGSNAG2` 7G-Tronic has 2016, 2017, and 2019 versions; `HERMES` telematics has 6 versions spanning 2016–2020)

### CLI Catalog Commands
```bash
# Display overall CBF database and deduplication statistics
sterngate cbf stats

# Search ECUs by name or chassis keyword
sterngate cbf search EGS
sterngate cbf search W211

# Detailed inspection of an ECU (canonical file, protocol, CAN IDs, DTC count, chassis list)
sterngate cbf inspect VGSNAG2
sterngate cbf inspect EDC16
```

### Re-analyzing or Updating the Catalog
```bash
python3 scripts/cbf_dedup_analyzer.py \
  --cbf-dir data/cbf \
  --output data/cbf_catalog.json
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

