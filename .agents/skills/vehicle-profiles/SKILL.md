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

## 2. Converting Legacy CBF Files to Sterngate JSON

Legacy Mercedes tools use binary `.cbf` files (e.g., `CR4.cbf` for EDC16, `EGS52.cbf` for 722.6).

To convert a `.cbf` to Sterngate JSON:
1. Decompile `.cbf` using open-source tools such as `CaesarSuite` or `cbf-parser`:
   ```bash
   cbf-parser CR4.cbf --json cr4_raw.json
   ```
2. Extract the target DIDs, byte lengths, and linear conversion factors:
   $$\text{Physical Value} = \text{Raw} \times \text{slope} + \text{offset}$$
3. Validate against the schema:
   ```bash
   cargo test --package sterngate-core test_validate_profiles
   ```

---

## 3. Parameter Scaling Types

Sterngate supports:
- **Linear**: `slope * raw + offset`
- **Bitmask**: Extract specific bit flags (e.g. lockup clutch status `0x00 = Open`, `0x01 = Slipping`, `0x02 = Closed`)
- **Signed 16-bit**: Two's complement for negative values (e.g. injector quantity deviation $-5.0\text{ to }+5.0\text{ mm}^3$)

