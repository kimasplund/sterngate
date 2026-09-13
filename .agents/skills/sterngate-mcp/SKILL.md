---
name: sterngate-mcp
description: >-
  Guide for configuring, testing, and interacting with Sterngate via the Model Context Protocol (MCP).
  Use when connecting AI agents to vehicle diagnostics, telemetry, DTCs, or ECU inspections.
---

# Sterngate Model Context Protocol (MCP) Integration

Use this skill to configure and call Sterngate as an MCP server.

## 1. Connecting the MCP Server

Add Sterngate to your MCP configuration (e.g., `~/.gemini/config/mcp_config.json` or project `mcp_config.json`):

```json
{
  "mcpServers": {
    "sterngate": {
      "command": "cargo",
      "args": ["run", "--quiet", "--package", "sterngate-cli", "--", "mcp"],
      "env": {
        "RUST_LOG": "error",
        "STERNGATE_DEFAULT_PROFILE": "profiles/mercedes/w211_om646_edc16.json"
      }
    }
  }
}
```

Or when running the compiled release binary:
```json
{
  "mcpServers": {
    "sterngate": {
      "command": "/path/to/sterngate",
      "args": ["mcp"]
    }
  }
}
```

---

## 2. Available MCP Tools

| Tool | Parameters | Description |
| :--- | :--- | :--- |
| `sterngate_list_interfaces` | *(None)* | Enumerates available CAN interfaces (`can0`, `vcan0`, `mock`, J2534). |
| `sterngate_read_telemetry` | `interface` (opt) | Returns a real-time snapshot of engine, transmission, and chassis values (RPM, ATF temp, rail pressure, boost, cylinder balance). |
| `sterngate_read_dtc` | `module` (opt), `lang` (opt: `en`, `de`, `sv`) | Reads active and stored Diagnostic Trouble Codes (DTCs) with localized descriptions (English, authentic German, Swedish). |
| `sterngate_clear_dtc` | `module` (opt) | Clears DTC fault memory on the target module or entire gateway via Service 0x14. |
| `sterngate_read_parameter` | `parameter` (opt), `module` (opt) | Reads a specific sensor parameter (e.g. `Transmission Fluid Temp`, `0x2001`, `rail_pressure`). |
| `sterngate_inspect_ecu` | `module` (opt) | Fetches hardware ID, software revision, calibration ID, protocol, and VIN. |
| `sterngate_trigger_routine` | `routine_id`, `module`, `sub_function`, `lang` (opt) | Triggers UDS Service 0x31 actuator/diagnostic routines (fuel prime `0xFF01`, NMK reset `0x0201`, DPF regen `0x0202`) with zero-trust safety verification and localized feedback. |
| `sterngate_control_flight_recorder` | `action` (`start`/`stop`/`status`), `filename` (opt) | Controls high-frequency continuous CSV telemetry flight recording for track/tow/dyno logging. |
| `sterngate_list_profiles` | *(None)* | Dynamically discovers and lists all installed vehicle profiles in `profiles/` (15 models across Mercedes, VAG, and BMW). |
| `sterngate_search_cbf_catalog` | `query`, `limit` (opt) | Searches the canonical 2,055-file CBF catalog for ECUs, protocols, duplicate stats, and supported chassis. |
| `sterngate_inspect_cbf_ecu` | `ecu` | Inspects detailed diagnostic routing, CAN transmission/reception IDs, protocol, fault code counts, and cross-chassis compatibility for any ECU. |
| `sterngate_list_locales` | *(None)* | Lists supported UI and diagnostic languages (`en`, `de`, `sv`). |
| `sterngate_verify_flash_staging` | `target_module`, `expected_hw_id`, `sha256`, `crc32` | Evaluates a staged flash binary against safety checks (battery voltage $\ge 12.5\text{ V}$, CRC32, SHA256, HW match). |

---

## 3. Available MCP Resources

| URI | Name | Description |
| :--- | :--- | :--- |
| `sterngate://locales` | Supported Languages | List of active localization languages (`en`, `de`, `sv`). |
| `sterngate://cbf/stats` | CBF Database Statistics | Deduplication metrics for 2,055 CBF files (990 unique ECUs, 846 redundant copies). |
| `sterngate://profile/w211_om646` | W211 OM646 Profile | Full DID mappings, scaling equations, and module definitions for W211 CDI. |
| `sterngate://ecu/status` | ECU Bus Status | Active CAN interface, baudrate, battery voltage, and flasher lockout status. |

---

## 4. Testing MCP via CLI Stdio

You can test MCP JSON-RPC 2.0 communication directly from the terminal:

```bash
cargo run --package sterngate-cli -- mcp << 'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"sterngate_read_dtc","arguments":{"module":"EDC16","lang":"de"}}}
{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"sterngate://cbf/stats"}}
EOF
```

---

## 5. Diagnostic Workflows for AI Agents

When a user asks:
- *"Why is my transmission shifting roughly at 80°C?"*
  1. Call `sterngate_read_parameter` with parameter `trans_fluid_temp` and verify against 80°C level check window.
  2. Call `sterngate_read_dtc` targeting `EGS52`.
  3. Synthesize the findings and explain possible clutch adaptation or fluid level issues.
- *"Read fault codes in German:"*
  1. Call `sterngate_read_dtc` with `{"module": "EDC16", "lang": "de"}`.
  2. The tool returns authentic OEM descriptions (e.g. *Luftmassenmesser (LMM) Schaltkreis Fehlfunktion*).
- *"Bleed the fuel rail after fuel filter replacement:"*
  1. Call `sterngate_trigger_routine` with `{"routine_id": "0xFF01", "module": "EDC16", "sub_function": 1}`.
- *"Find which chassis use the 722.9 7G-Tronic transmission ECU:"*
  1. Call `sterngate_inspect_cbf_ecu` with `{"ecu": "VGSNAG2"}`.
  2. Inspect the returned `chassis_supported` array (e.g. W204, W212, W221, W164).


