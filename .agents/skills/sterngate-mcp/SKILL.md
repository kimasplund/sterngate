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
| `sterngate_list_interfaces` | *(None)* | Enumerates available CAN interfaces (`can0`, `vcan0`, `mock`). |
| `sterngate_read_telemetry` | `interface` (opt) | Returns a real-time snapshot of engine, transmission, and chassis values. |
| `sterngate_read_dtc` | `module` (opt) | Reads active and stored Diagnostic Trouble Codes (DTCs) with descriptions. |
| `sterngate_clear_dtc` | `module` (opt) | Clears DTC fault memory on the target module or entire gateway. |
| `sterngate_read_parameter` | `name` or `did` | Reads a specific sensor parameter (e.g. `Transmission Fluid Temp`, `0x2001`). |
| `sterngate_write_parameter`| `did`, `hex_value` | Performs variant coding / parameter write after security handshake. |
| `sterngate_inspect_ecu` | `module` | Fetches hardware ID, software revision, and calibration ID. |
| `sterngate_trigger_routine` | `routine_id`, `module`, `sub_function` | Triggers UDS Service 0x31 actuator/diagnostic routines (fuel prime, NMK reset, DPF regen). |
| `sterngate_control_flight_recorder` | `action`, `filename` | Controls high-frequency continuous CSV telemetry flight recording (start, stop, status). |
| `sterngate_list_profiles` | *(None)* | Lists installed vehicle profiles in `profiles/`. |
| `sterngate_verify_flash_staging` | `manifest_path` | Evaluates a staged flash binary against safety checks (voltage, CRC, HW). |

---

## 3. Testing MCP via CLI Stdio

You can test MCP JSON-RPC 2.0 communication directly from the terminal:

```bash
cargo run --package sterngate-cli -- mcp << 'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"sterngate_read_telemetry","arguments":{}}}
EOF
```

---

## 4. Diagnostic Workflows for AI Agents

When a user asks:
- *"Why is my transmission shifting roughly at 80°C?"*
  1. Call `sterngate_read_parameter` for `Transmission Fluid Temp` and `TCC Lockup Slip`.
  2. Call `sterngate_read_dtc` targeting `EGS52`.
  3. Synthesize the findings and explain possible clutch adaptation or fluid level issues.
- *"Check cylinder smooth-running injector corrections:"*
  1. Call `sterngate_read_telemetry`.
  2. Inspect `Cylinder 1-4 Correction` (normal range: $-2.0\text{ to }+2.0\text{ mm}^3/\text{stroke}$).
  3. Flag any injector exceeding $\pm 3.0\text{ mm}^3/\text{stroke}$ as potentially leaky or worn.

