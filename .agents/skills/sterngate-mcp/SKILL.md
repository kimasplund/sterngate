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
| `sterngate_read_parameter` | `parameter` (opt), `module` (opt), `lang` (opt: `en`, `de`, `sv`) | Reads a specific sensor parameter (e.g. `Transmission Fluid Temp`, `0x2001`, `rail_pressure`, `tcc_slip_rpm`) with localized naming. |
| `sterngate_inspect_ecu` | `module` (opt) | Fetches hardware ID, software revision, calibration ID, protocol, and VIN. |
| `sterngate_trigger_routine` | `routine_id`, `module`, `sub_function`, `lang` (opt) | Triggers UDS Service 0x31 actuator/diagnostic routines (fuel prime `0xFF01`, NMK reset `0x0201`, DPF regen `0x0202`) with zero-trust safety verification and localized feedback. |
| `sterngate_control_flight_recorder` | `action` (`start`/`stop`/`status`), `filename` (opt) | Controls high-frequency continuous CSV telemetry flight recording for track/tow/dyno logging. |
| `sterngate_list_profiles` | *(None)* | Dynamically discovers and lists all installed vehicle profiles in `profiles/` (15 models across Mercedes, VAG, and BMW). |
| `sterngate_search_ecu_catalog` | `query`, `limit` (opt) | Searches the canonical 990-ECU database for ECUs, protocols, and supported vehicle platforms. |
| `sterngate_inspect_ecu_definition` | `ecu` | Inspects detailed diagnostic routing, CAN transmission/reception IDs, protocol, fault code counts, and cross-chassis compatibility for any ECU. |
| `sterngate_list_locales` | *(None)* | Lists supported UI and diagnostic languages (`en`, `de`, `sv`). |
| `sterngate_scan_vehicle` | `lang` (opt: `en`, `de`, `sv`), `save_to_garage` (opt) | Executes full quick scan across all gateway ECUs, decodes VIN, reads DTCs, samples vitals, and commits snapshot to vehicle's Git garage repo. |
| `sterngate_list_vehicles` | *(None)* | Lists all recognized vehicles stored in local Git garage by VIN with model, scan count, and last scanned date. |
| `sterngate_analyze_suspension_leak` | `duration_min` (opt), `left_rear_start_mm` (opt), `left_rear_end_mm` (opt), `right_rear_start_mm` (opt), `right_rear_end_mm` (opt), `compressor_run_time_sec` (opt), `compressor_duty_cycle_pct` (opt) | Evaluates Mercedes S211 rear air suspension (ENR) or W211 AIRMATIC for pneumatic leaks, drop rate mm/h, L/R height asymmetry, and compressor duty cycle strain. |
| `sterngate_protect_compressor` | `action` (`inhibit`, `restore`, `workshop`), `reason` (opt) | Controls active compressor protection on Mercedes S211 ENR / W211 AIRMATIC to prevent compressor motor burnout and relay welding during air leaks. |
| `sterngate_compare_drive_runs` | `run_a` (opt), `run_b` (opt), `baseline_...` (opt), `target_...` (opt) | Performs A/B comparative benchmark between two drive telemetry runs to evaluate whether parameter/mechanical changes were beneficial (fuel consumption, TCC slip, boost). |
| `sterngate_verify_flash_staging` | `target_module`, `expected_hw_id`, `sha256`, `crc32` | Evaluates a staged flash binary against safety checks (battery voltage $\ge 12.5\text{ V}$, CRC32, SHA256, HW match). |

---

## 3. Available MCP Resources

| URI | Name | Description |
| :--- | :--- | :--- |
| `sterngate://locales` | Supported Languages | List of active localization languages (`en`, `de`, `sv`). |
| `sterngate://ecu/catalog` | Automotive ECU Catalog | Diagnostic metadata for 990 unique ECUs across multiple vehicle architectures. |
| `sterngate://profile/w211_om646` | W211 OM646 Profile | Full DID mappings, scaling equations, and module definitions for W211 CDI. |
| `sterngate://ecu/status` | ECU Bus Status | Active CAN interface, baudrate, battery voltage, and flasher lockout status. |
| `sterngate://garage/vehicles` | Vehicle Garage Database | List of tracked vehicles with decoded VINs, installed ECUs, and Git configuration history. |

---

## 4. Testing MCP via CLI Stdio

You can test MCP JSON-RPC 2.0 communication directly from the terminal:

```bash
cargo run --package sterngate-cli -- mcp << 'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"sterngate_read_dtc","arguments":{"module":"EDC16","lang":"de"}}}
{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"sterngate://ecu/catalog"}}
EOF
```

---

## 5. Diagnostic Workflows for AI Agents

When a user asks:
- *"Perform a complete health check on my car and save it:"*
  1. Call `sterngate_scan_vehicle` with `{"save_to_garage": true, "lang": "en"}`.
  2. The tool interrogates all gateway ECUs, decodes the VIN (e.g. S211 Estate OM646), creates an atomic Git commit in `data/vehicles/<VIN>/`, and returns all DTCs, vitals, and issues.
- *"My S211 wagon is sagging overnight on the rear left side, is the airbag leaking?"*
  1. Call `sterngate_analyze_suspension_leak` with recorded height measurements or live telemetry.
  2. The tool calculates height drop rate mm/hour, asymmetry, and compressor duty cycle. If $> 10\text{ mm/h}$, it flags `CriticalLeak` and recommends inspecting rear pneumatic bellows (`A 211 320 09 25`) and relay (`A 002 542 72 19`).
- *"My rear air suspension is leaking and I need to drive to the shop without burning out the compressor:"*
  1. Call `sterngate_protect_compressor` with `{"action": "inhibit", "reason": "Prevent thermal overload while driving"}` or `{"action": "workshop"}`.
  2. The tool triggers UDS Routine `0x0210` (Safe Mode Inhibit) or `0x0211` (Workshop Mode), de-energizing the compressor relay circuit to prevent the sustained $>30\text{A}$ overload that welds relay contacts closed.
- *"Did my adaptation reset or tune improve fuel consumption?"*
  1. Call `sterngate_compare_drive_runs` with baseline and target drive statistics.
  2. The tool returns the fuel consumption delta $L/100\text{km}$, lockup clutch slip delta, and an overall verdict (`Beneficial`, `Neutral`, or `Detrimental`).
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
  1. Call `sterngate_inspect_ecu_definition` with `{"ecu": "VGSNAG2"}`.
  2. Inspect the returned `chassis_supported` array (e.g. W204, W212, W221, W164).


