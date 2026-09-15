use serde_json::{json, Value};

pub fn get_tools_list() -> Value {
    json!([
        {
            "name": "sterngate_list_interfaces",
            "description": "List all detected physical, virtual, and pass-thru vehicle communication interfaces (SocketCAN can0/vcan0, Virtual Mock, J2534).",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "sterngate_read_telemetry",
            "description": "Capture a live snapshot of vehicle powertrain telemetry including Engine RPM, Coolant Temp, Transmission Fluid Temp (722.6), Common Rail Pressure, Boost (MAP), and cylinder smooth-running injector balances.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "interface": {
                        "type": "string",
                        "description": "CAN interface name (default: virtual mock simulator or can0)"
                    }
                }
            }
        },
        {
            "name": "sterngate_read_dtc",
            "description": "Read Diagnostic Trouble Codes (DTCs) from the vehicle gateway and target modules (e.g. Bosch EDC16 engine, EGS52 transmission, Airmatic). Returns standard alphanumeric codes with localized descriptions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": {
                        "type": "string",
                        "description": "Target module name (e.g. EDC16, EGS52, CGW). Defaults to EDC16.",
                        "default": "EDC16"
                    },
                    "lang": {
                        "type": "string",
                        "description": "Language for DTC descriptions: 'en' (English), 'de' (German / Daimler OEM), 'sv' (Swedish). Default: 'en'",
                        "enum": ["en", "de", "sv"],
                        "default": "en"
                    }
                }
            }
        },
        {
            "name": "sterngate_clear_dtc",
            "description": "Clear diagnostic trouble codes and reset fault memory in the target ECU module.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": {
                        "type": "string",
                        "description": "Target module to clear (e.g. EDC16, EGS52).",
                        "default": "EDC16"
                    }
                }
            }
        },
        {
            "name": "sterngate_read_parameter",
            "description": "Read a specific manufacturer DID (Data Identifier) or friendly parameter name (e.g., 'trans_fluid_temp', '0x2001', 'rail_pressure').",
            "inputSchema": {
                "type": "object",
                "required": ["parameter"],
                "properties": {
                    "parameter": {
                        "type": "string",
                        "description": "Parameter ID or Hex DID (e.g., 'trans_fluid_temp' or '0x2001')"
                    },
                    "module": {
                        "type": "string",
                        "description": "Target ECU module (e.g., 'EDC16', 'EGS52')",
                        "default": "EDC16"
                    },
                    "lang": {
                        "type": "string",
                        "description": "Language for parameter name ('en', 'de', 'sv')",
                        "default": "en"
                    }
                }
            }
        },
        {
            "name": "sterngate_inspect_ecu",
            "description": "Query ECU identification info: Hardware number, Software revision, Calibration ID, and VIN.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": {
                        "type": "string",
                        "description": "Target module to inspect",
                        "default": "EDC16"
                    }
                }
            }
        },
        {
            "name": "sterngate_list_profiles",
            "description": "List all installed vehicle profile definitions in the repository.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "sterngate_verify_flash_staging",
            "description": "Evaluate an ECU firmware flashing package against strict automotive pre-flight safety gates (battery voltage >= 12.5V, SHA256 checksum, Bosch CRC32, HW/SW calibration match).",
            "inputSchema": {
                "type": "object",
                "required": ["target_module", "expected_hw_id", "sha256", "crc32"],
                "properties": {
                    "target_module": { "type": "string" },
                    "expected_hw_id": { "type": "string" },
                    "sha256": { "type": "string" },
                    "crc32": { "type": "integer" }
                }
            }
        },
        {
            "name": "sterngate_trigger_routine",
            "description": "Trigger an automotive ECU diagnostic routine (UDS Service 0x31 RoutineControl) such as fuel pump prime (0xFF01), reset zero-quantity injector adaptations (0x0201), trigger DPF regeneration (0x0202), throttle/EGR relearn (0x0203), or SBC brake hydraulic bleed (0x0205) with zero-trust safety verification.",
            "inputSchema": {
                "type": "object",
                "required": ["routine_id"],
                "properties": {
                    "routine_id": {
                        "type": "string",
                        "description": "Hex routine identifier (e.g., '0xFF01', '0x0201', '0x0202', '0x0203', '0x0205')"
                    },
                    "module": {
                        "type": "string",
                        "description": "Target ECU module (e.g., 'EDC16', 'EGS52'). Default: EDC16",
                        "default": "EDC16"
                    },
                    "sub_function": {
                        "type": "integer",
                        "description": "Routine sub-function: 1 for startRoutine, 2 for stopRoutine, 3 for requestResults. Default: 1",
                        "default": 1
                    },
                    "lang": {
                        "type": "string",
                        "description": "Language for routine name and status feedback: 'en' (English), 'de' (German), 'sv' (Swedish). Default: 'en'",
                        "enum": ["en", "de", "sv"],
                        "default": "en"
                    }
                }
            }
        },
        {
            "name": "sterngate_control_flight_recorder",
            "description": "Control the high-frequency continuous flight recorder for track/tow/dyno telemetry CSV logging (start, stop, or query status).",
            "inputSchema": {
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["start", "stop", "status"],
                        "description": "Action to perform: 'start' initiates CSV flight recording, 'stop' flushes and ends recording, 'status' returns current state and row count."
                    },
                    "filename": {
                        "type": "string",
                        "description": "Optional custom filename for CSV telemetry log (e.g., 'dyno_pull_stage2.csv')"
                    }
                }
            }
        },
        {
            "name": "sterngate_search_ecu_catalog",
            "description": "Search the canonical Automotive ECU catalog (990 unique ECUs across multiple vehicle architectures) by ECU name (e.g. 'EGS52', 'CR3', 'MED177', 'VGSNAG2', 'DQ250', 'DDE6') or chassis family. Returns canonical arbitration CAN IDs, protocol, and cross-chassis compatibility.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "ECU name or chassis keyword (e.g. 'EGS52', 'CR3', 'W211')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of search results to return (default: 25)",
                        "default": 25
                    }
                }
            }
        },
        {
            "name": "sterngate_inspect_ecu_definition",
            "description": "Inspect detailed diagnostic routing, CAN transmission/reception IDs, protocol, fault code count, presentation count, and supported chassis for a specific ECU in the Sterngate ECU database (e.g. 'EGS52', 'CR3', 'VGSNAG2').",
            "inputSchema": {
                "type": "object",
                "required": ["ecu"],
                "properties": {
                    "ecu": {
                        "type": "string",
                        "description": "ECU name (e.g. 'EGS52', 'CR3', 'MED177', 'VGSNAG2')"
                    }
                }
            }
        },
        {
            "name": "sterngate_list_locales",
            "description": "List supported UI, diagnostic, and fault code languages in Sterngate (English, authentic Daimler OEM German, Swedish).",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "sterngate_scan_vehicle",
            "description": "Execute a full vehicle quick scan across all gateway ECUs, decode VIN, read DTCs, capture baseline vitals, and automatically record vehicle into git-tracked garage.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "lang": {
                        "type": "string",
                        "enum": ["en", "de", "sv"],
                        "description": "Language for diagnostic report (default: 'en')",
                        "default": "en"
                    },
                    "save_to_garage": {
                        "type": "boolean",
                        "description": "Whether to synchronize scan into local vehicle garage git repo (default: true)",
                        "default": true
                    }
                }
            }
        },
        {
            "name": "sterngate_list_vehicles",
            "description": "List all recognized vehicles saved in the local Sterngate garage by VIN with model, scan count, and last scanned date.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "sterngate_analyze_suspension_leak",
            "description": "Evaluate Mercedes-Benz S211 rear air suspension (ENR) or W211 AIRMATIC for pneumatic leaks, height drop rate, and compressor duty cycle strain.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "sterngate_compare_drive_runs",
            "description": "Perform an A/B comparative benchmark between two drive telemetry runs to evaluate whether a parameter or mechanical change was beneficial for fuel consumption and transmission slip.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "sterngate_protect_compressor",
            "description": "Protect or force disable/enable the Mercedes-Benz S211 rear air suspension (ENR) or W211 AIRMATIC compressor to prevent motor burnout, thermal overload, or relay contact welding during air leaks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["inhibit", "workshop", "restore", "status"],
                        "description": "Action: 'inhibit' (force cutoff/safe mode via routine 0x0210), 'workshop' (transport mode/leveling locked via routine 0x0211), 'restore' (normal operation via routine 0x0212), or 'status' (query watchdog state)",
                        "default": "inhibit"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Reason for override (e.g. 'Driver safe mode: leaking rear left bellow')"
                    }
                },
                "required": ["action"]
            }
        },
        {
            "name": "sterngate_control_abc_limiter",
            "description": "Control ABC (Active Body Control) hydraulic surge limiter and isolation routines (Routine 0x0220: pressure fallback dump to 120 bar safe mode; Routine 0x0221: strut isolation valve lock; Routine 0x0222: restore normal active dynamic damping).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "ABC containment action: 'dump' (120 bar safe fallback), 'lock' (strut isolation valves locked), 'restore' (normal active damping)",
                        "enum": ["dump", "lock", "restore"]
                    }
                },
                "required": ["action"]
            }
        },
        {
            "name": "sterngate_check_cascade_warnings",
            "description": "Inspect and evaluate vehicle vitals and diagnostics against 13 known Mercedes-Benz 'Cascade of Death' failure modes (SBC accumulator loss, injector copper seal Black Death, 722.6 pilot bushing wicking, TCC lockup slip, DPF differential drift/M55 short, cam magnet oil wicking, air suspension compressor burnout, ABC pulsation damper surge, ESL steering lock DC motor seizure, M272/M273 balance shaft wear, Valeo radiator glycol contamination, SAM water ingress & parasitic drain, OM642 oil cooler V-valley leak).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sbc_accumulator_pressure_bar": {
                        "type": "number",
                        "description": "SBC pre-charge accumulator pressure in bar (nominal 70-85 bar, critical <55 bar)"
                    },
                    "max_cylinder_balance_trim_mm3": {
                        "type": "number",
                        "description": "Maximum smooth-running cylinder balance trim in mm³/hub (nominal <1.5 mm³, critical >3.5 mm³)"
                    },
                    "tcc_slip_rpm": {
                        "type": "number",
                        "description": "Torque converter clutch slip in RPM during lockup (nominal <30 RPM, critical >60 RPM)"
                    },
                    "compressor_continuous_run_sec": {
                        "type": "number",
                        "description": "Continuous air suspension compressor runtime in seconds (nominal <25s, critical >40s)"
                    },
                    "suspension_height_drop_rate_mm_h": {
                        "type": "number",
                        "description": "Stationary rear suspension height drop rate in mm/hour (nominal <2 mm/h, critical >10 mm/h)"
                    },
                    "abc_pressure_ripple_bar": {
                        "type": "number",
                        "description": "ABC hydraulic line pressure ripple amplitude in bar (nominal <5 bar, critical >25 bar)"
                    },
                    "esl_unlock_duration_ms": {
                        "type": "number",
                        "description": "Electronic steering lock (ESL/ELV) motor unlock duration in ms (nominal 120-220ms, critical >500ms)"
                    },
                    "cam_phase_deviation_deg": {
                        "type": "number",
                        "description": "Camshaft phase angle deviation in crank degrees (nominal <1.0°, critical >3.2°)"
                    },
                    "tcc_slip_oscillation_hz": {
                        "type": "number",
                        "description": "Harmonic TCC slip oscillation frequency in Hz (glycol contamination shudder 4-12 Hz)"
                    },
                    "can_sleep_delay_seconds": {
                        "type": "number",
                        "description": "Interior CAN-B bus sleep delay in seconds (nominal <30s, critical >120s)"
                    },
                    "dynamic_oil_loss_rate_mm_100km": {
                        "type": "number",
                        "description": "Highway dynamic oil level consumption rate in mm/100km (nominal <0.05, critical >0.25)"
                    }
                }
            }
        },
        {
            "name": "sterngate_discover_ecus",
            "description": "Probes the vehicle CAN bus across an arbitration ID range (e.g. 0x7E0..=0x7EF or 0x700..=0x7EF), interrogates responsive nodes with standard UDS identification DIDs (part number, HW/SW version, VIN, system name), and automatically matches them against the 990-ECU database catalog.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "start_id": {
                        "type": "integer",
                        "description": "Starting CAN arbitration ID (default: 0x7E0 / 2016)",
                        "default": 2016
                    },
                    "end_id": {
                        "type": "integer",
                        "description": "Ending CAN arbitration ID (default: 0x7EF / 2031)",
                        "default": 2031
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Timeout per queried CAN ID in milliseconds (default: 20)",
                        "default": 20
                    }
                }
            }
        },
        {
            "name": "sterngate_service_routine",
            "description": "Execute safety-critical automotive workshop service routines: SBC brake pad deactivation (dumps 160 bar pressure, locks wake-up triggers for safe brake service) / reactivation; Common Rail injector IMA calibration code read/write (with automatic garage git commit); Air suspension corner inflation/deflation and zero-level sensor calibration.",
            "inputSchema": {
                "type": "object",
                "required": ["routine"],
                "properties": {
                    "routine": {
                        "type": "string",
                        "enum": ["sbc_deactivate", "sbc_reactivate", "read_ima", "write_ima", "suspension_corner"],
                        "description": "Routine type to execute"
                    },
                    "cylinder": {
                        "type": "integer",
                        "description": "Cylinder number (1-8) for read_ima / write_ima"
                    },
                    "code": {
                        "type": "string",
                        "description": "6 or 7 character alphanumeric IMA calibration code for write_ima (e.g. '7B8HNA')"
                    },
                    "corner": {
                        "type": "string",
                        "enum": ["FrontLeft", "FrontRight", "RearLeft", "RearRight", "BothRear", "AllCorners"],
                        "description": "Air suspension corner to actuate"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["inflate", "deflate", "calibrate_zero"],
                        "description": "Suspension corner action"
                    },
                    "vin": {
                        "type": "string",
                        "description": "Vehicle VIN for recording IMA coding mutations in vehicle garage git repository"
                    }
                }
            }
        },
        {
            "name": "sterngate_flash_ecu",
            "description": "Simulate and execute safe detached ECU firmware flashing with battery voltage interlock (>= 12.5V), cryptographic package staging verification, and block transfer simulation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target_module": {
                        "type": "string",
                        "description": "Target ECU module (default: 'EDC16')",
                        "default": "EDC16"
                    },
                    "battery_voltage": {
                        "type": "number",
                        "description": "Simulated battery voltage in volts (fails if < 12.5V)",
                        "default": 13.8
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "If true, only runs pre-flight safety gates without starting transfer",
                        "default": false
                    }
                }
            }
        },
        {
            "name": "sterngate_export_report",
            "description": "Run a full vehicle diagnostic quick-test and export a high-resolution, self-contained HTML diagnostic report with localized descriptions, vitals, ECU inventory, and cascade risk warnings.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "lang": {
                        "type": "string",
                        "enum": ["en", "de", "sv"],
                        "description": "Language for the HTML diagnostic report (default: 'en')",
                        "default": "en"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Optional file path to save HTML report on local filesystem (e.g. 'diagnostic_report.html')"
                    }
                }
            }
        },
        {
            "name": "sterngate_guided_workflow",
            "description": "Execute automotive guided workflows and one-click quick mods: VMax speed limiter (DID 0x0110), seatbelt acoustic chime (DID 0x0201), tank liters Restliteranzeige (DID 0x0205), cornering fog lights (DID 0x0310), ECO start-stop memory (DID 0x0320), EGR soot air mass optimization, and AdBlue 800km emergency lockout reset. Automatically commits configuration to vehicle garage git history.",
            "inputSchema": {
                "type": "object",
                "required": ["workflow"],
                "properties": {
                    "workflow": {
                        "type": "string",
                        "enum": ["vmax", "seatbelt_chime", "tank_liters", "cornering_lights", "eco_start_stop", "egr_optimize", "adblue_reset"],
                        "description": "Target workflow or quick mod to execute"
                    },
                    "speed_limit_kmh": {
                        "type": "integer",
                        "description": "Speed limit in km/h for 'vmax' (e.g. 210, 250, 280, 300). Default: 250",
                        "default": 250
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Enable/disable flag for seatbelt_chime (true=audible chime on, false=muted), tank_liters, or cornering_lights. Default: true",
                        "default": true
                    },
                    "eco_mode": {
                        "type": "string",
                        "enum": ["remember", "disabled", "always_on"],
                        "description": "ECO Start-Stop mode: 'remember' (driver last state), 'disabled' (inverted), 'always_on' (factory standard). Default: 'remember'",
                        "default": "remember"
                    },
                    "vin": {
                        "type": "string",
                        "description": "Vehicle VIN for recording mutation in vehicle garage git repository"
                    }
                }
            }
        },
        {
            "name": "sterngate_vault_scan",
            "description": "Scan local disk directory for firmware binaries (.bin, .rom, .cff, .smr-f, .fls), extract embedded Bosch HW/SW numbers, and cross-reference against connected ECU hardware to discover calibration upgrades.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Local directory path to scan (default: 'firmware_vault')",
                        "default": "firmware_vault"
                    },
                    "hw_id": {
                        "type": "string",
                        "description": "Target ECU hardware number to check for upgrades (e.g. '0281012224')"
                    },
                    "sw_id": {
                        "type": "string",
                        "description": "Current ECU calibration software number (e.g. '1037365000')"
                    }
                }
            }
        },
        {
            "name": "sterngate_import_profiles",
            "description": "Batch ingest and convert Daimler CBF and SMR-D diagnostic database files into native Sterngate JSON profiles, mapping CAN IDs using the 990-ECU canonical catalog.",
            "inputSchema": {
                "type": "object",
                "required": ["input_path"],
                "properties": {
                    "input_path": {
                        "type": "string",
                        "description": "Path to CBF/SMR-D archive file or extracted directory"
                    },
                    "output_dir": {
                        "type": "string",
                        "description": "Target directory for generated Sterngate JSON profiles (default: 'profiles')",
                        "default": "profiles"
                    }
                }
            }
        },
        {
            "name": "sterngate_inspect_community_mod",
            "description": "Inspect and cryptographically validate a community mod package (.sgmod file, raw JSON, or copy-pasteable ASCII armored text). Verifies CRC32/SHA256 checksums and automatically executes Reed-Solomon Forward Error Correction to repair any forum or chat bitflips.",
            "inputSchema": {
                "type": "object",
                "required": ["mod_content"],
                "properties": {
                    "mod_content": {
                        "type": "string",
                        "description": "Path to .sgmod file, or raw JSON, or ASCII armored text block (-----BEGIN STERNGATE COMMUNITY MOD-----)"
                    },
                    "vin": {
                        "type": "string",
                        "description": "Vehicle Identification Number (VIN) to check chassis compatibility against"
                    },
                    "battery_voltage": {
                        "type": "number",
                        "description": "Current measured battery voltage to verify against safety interlocks (e.g. 12.6)"
                    }
                }
            }
        },
        {
            "name": "sterngate_apply_community_mod",
            "description": "Apply a verified community mod or tuning parameter package to the connected vehicle. Enforces map provenance, integrity, chassis, HW ID whitelist, the 12.5 V floor for flash writes and live byte preconditions; creates an atomic Git garage snapshot; applies DID writes with bitmask preservation. No bypass flag exists over MCP.",
            "inputSchema": {
                "type": "object",
                "required": ["mod_content"],
                "properties": {
                    "mod_content": {
                        "type": "string",
                        "description": "Path to .sgmod file, raw JSON, or ASCII armored text block"
                    },
                    "vin": {
                        "type": "string",
                        "description": "Target vehicle VIN (defaults to first stored garage vehicle or WDB211)"
                    },
                    "battery_voltage": {
                        "type": "number",
                        "description": "Live battery voltage reading (defaults to 13.0V if not provided)"
                    }
                }
            }
        },
        {
            "name": "sterngate_create_community_mod",
            "description": "Author a shareable, fault-tolerant community mod package (.sgmod and ASCII armored text). Encodes parameters, computes CRC32 and SHA256 hashes, and generates Reed-Solomon (255, 239) error correction parity blocks.",
            "inputSchema": {
                "type": "object",
                "required": ["name", "author", "description", "ecu", "did", "data"],
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Human-readable mod title (e.g. 'AMG Instrument Needle Sweep')"
                    },
                    "author": {
                        "type": "string",
                        "description": "Author name, forum handle, or organization"
                    },
                    "description": {
                        "type": "string",
                        "description": "Detailed explanation of what the mod changes"
                    },
                    "category": {
                        "type": "string",
                        "description": "Mod category: 'performance', 'transmission', 'comfort', 'lighting', 'brakes', 'emissions', 'retrofit'",
                        "default": "comfort"
                    },
                    "risk_level": {
                        "type": "string",
                        "description": "Risk level: 'low', 'moderate', 'high'",
                        "default": "low"
                    },
                    "chassis": {
                        "type": "string",
                        "description": "Target chassis (e.g. 'W211', 'W204', 'W221', or 'Universal')",
                        "default": "W211"
                    },
                    "ecu": {
                        "type": "string",
                        "description": "Target ECU module name (e.g. 'IC_211', 'EDC16', 'EGS52')"
                    },
                    "did": {
                        "type": "string",
                        "description": "Data Identifier in hex (e.g. '0x01B0' or '01B0')"
                    },
                    "data": {
                        "type": "string",
                        "description": "Payload bytes in hex (e.g. '01FF02')"
                    },
                    "bitmask": {
                        "type": "string",
                        "description": "Optional bitmask in hex (e.g. '00FF00') where 1=replace bit, 0=keep original bit"
                    },
                    "expected_original_data": {
                        "type": "string",
                        "description": "Optional precondition original bytes in hex to verify before applying"
                    },
                    "min_voltage": {
                        "type": "number",
                        "description": "Minimum battery voltage required to apply (default: 12.0)",
                        "default": 12.0
                    },
                    "instructions": {
                        "type": "string",
                        "description": "Optional post-install instructions or prerequisites"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Optional file path to save the .sgmod file"
                    }
                }
            }
        },
        {
            "name": "sterngate_scan_rom_maps",
            "description": "Scan an ECU binary ROM dump (e.g. Bosch EDC16 2MB) using heuristic pattern matching to detect calibration maps (Driver's Wish, Boost, Torque Limiter, Smoke Limiter, Rail Pressure, EGR Hysteresis, SVBL, DTC tables), extract Bosch HW/SW identifiers, and verify MPC5xx block checksums.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rom_path": {
                        "type": "string",
                        "description": "File path to the ECU ROM binary dump"
                    },
                    "rom_base64": {
                        "type": "string",
                        "description": "Base64-encoded bytes of the ECU ROM binary dump"
                    }
                }
            }
        },
        {
            "name": "sterngate_generate_stage_tune",
            "description": "Generate a Stage 1 (+18% torque, +120 mbar boost, +50 bar rail) or Stage 2 (+25% torque, +200 mbar boost, DPF delete, EGR hysteresis zeroing, DTC kill) tuning package as a self-healing .sgmod file from an ECU ROM dump.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rom_path": {
                        "type": "string",
                        "description": "File path to the ECU ROM binary dump"
                    },
                    "rom_base64": {
                        "type": "string",
                        "description": "Base64-encoded bytes of the ECU ROM binary dump"
                    },
                    "stage": {
                        "type": "integer",
                        "description": "Tuning stage: 1 (conservative street) or 2 (aggressive + deletes). Default: 1",
                        "default": 1
                    },
                    "chassis": {
                        "type": "string",
                        "description": "Target chassis (e.g. 'W211 E280 CDI'). Default: 'W211'",
                        "default": "W211"
                    },
                    "ecu_name": {
                        "type": "string",
                        "description": "Target ECU name (e.g. 'EDC16CP31'). Default: 'EDC16CP31'",
                        "default": "EDC16CP31"
                    },
                    "author": {
                        "type": "string",
                        "description": "Author or tuning workshop name. Default: 'Sterngate Tuner'",
                        "default": "Sterngate Tuner"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Optional file path to save the generated .sgmod file"
                    }
                }
            }
        },
        {
            "name": "sterngate_kill_dtc",
            "description": "Generate a standalone DTC suppression .sgmod package for specific fault codes (e.g. ['P0401', 'P2002']) by locating DTC fault path tables in an ECU ROM dump and zeroing their enable masks.",
            "inputSchema": {
                "type": "object",
                "required": ["p_codes"],
                "properties": {
                    "rom_path": {
                        "type": "string",
                        "description": "File path to the ECU ROM binary dump"
                    },
                    "rom_base64": {
                        "type": "string",
                        "description": "Base64-encoded bytes of the ECU ROM binary dump"
                    },
                    "p_codes": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of standard alphanumeric OBD-II DTC codes to suppress (e.g. ['P0401', 'P2002', 'P0101'])"
                    },
                    "chassis": {
                        "type": "string",
                        "description": "Target chassis (e.g. 'W211'). Default: 'W211'",
                        "default": "W211"
                    },
                    "ecu_name": {
                        "type": "string",
                        "description": "Target ECU name (e.g. 'EDC16'). Default: 'EDC16'",
                        "default": "EDC16"
                    },
                    "author": {
                        "type": "string",
                        "description": "Author name. Default: 'Sterngate Tuner'",
                        "default": "Sterngate Tuner"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Optional file path to save the generated .sgmod file"
                    }
                }
            }
        },
        {
            "name": "sterngate_solve_checksum",
            "description": "Verify and optionally recalculate Bosch MPC5xx partitioned 32-bit block checksums and inverted complement pairs on an ECU ROM binary dump.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rom_path": {
                        "type": "string",
                        "description": "File path to the ECU ROM binary dump"
                    },
                    "rom_base64": {
                        "type": "string",
                        "description": "Base64-encoded bytes of the ECU ROM binary dump"
                    },
                    "fix": {
                        "type": "boolean",
                        "description": "If true, recalculates and updates all invalid block checksums in the ROM. Default: false",
                        "default": false
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Optional file path to write the patched ROM binary (if fix is true)"
                    }
                }
            }
        },
        {
            "name": "sterngate_search_workshop_routines",
            "description": "Search 1,523+ Mercedes-Benz OEM workshop actuator and diagnostic service routines (0x31 RoutineControl) by keyword, German/English description, routine ID, or ECU module name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search term (routine ID, German/English description, or category)"
                    },
                    "ecu": {
                        "type": "string",
                        "description": "Filter routines by ECU module name (e.g. CR4, EDC16, ESP, AIRMATIC)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of routines to return. Default: 25",
                        "default": 25
                    }
                }
            }
        },
        {
            "name": "sterngate_execute_service_routine",
            "description": "Execute a factory workshop actuator or service routine (Service 0x31 RoutineControl) on a target ECU with optional data payload bytes.",
            "inputSchema": {
                "type": "object",
                "required": ["routine_id"],
                "properties": {
                    "routine_id": {
                        "type": "string",
                        "description": "Routine identifier in hex (e.g. '0x0305', '0xFF01')"
                    },
                    "ecu": {
                        "type": "string",
                        "description": "Target ECU module (e.g. 'EDC16', 'CR4', 'ESP'). Default: 'EDC16'",
                        "default": "EDC16"
                    },
                    "data_hex": {
                        "type": "string",
                        "description": "Optional hex payload data bytes (e.g. '01FF')"
                    },
                    "tx_id": {
                        "type": "integer",
                        "description": "Optional CAN Tx arbitration ID (e.g. 0x7E0)"
                    },
                    "rx_id": {
                        "type": "integer",
                        "description": "Optional CAN Rx arbitration ID (e.g. 0x7E8)"
                    }
                }
            }
        },
        {
            "name": "sterngate_search_variant_coding_dids",
            "description": "Search 3,155+ Mercedes-Benz factory variant coding parameters and Data Identifiers (0x2E WriteDataByIdentifier) across 367 ECUs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search term (DID hex, parameter name, or description)"
                    },
                    "ecu": {
                        "type": "string",
                        "description": "Filter DIDs by ECU module name"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of DIDs to return. Default: 25",
                        "default": 25
                    }
                }
            }
        },
        {
            "name": "sterngate_adapt_donor_ecu_vin",
            "description": "Perform automated Donor Replacement ECU Re-VIN Adaptation: validates ISO 3779 17-char VIN, unlocks ECU via SecurityAccess (0x27), writes new VIN to 0xF190 (0x2E), verifies readback, and commits event to local Git garage.",
            "inputSchema": {
                "type": "object",
                "required": ["ecu", "new_vin"],
                "properties": {
                    "ecu": {
                        "type": "string",
                        "description": "Target replacement ECU module name (e.g. 'CR4', 'EDC16', 'MED17')"
                    },
                    "new_vin": {
                        "type": "string",
                        "description": "New 17-character vehicle identification number (VIN)"
                    },
                    "security_level": {
                        "type": "integer",
                        "description": "Optional SecurityAccess level override (e.g. 1, 3, 5, 9, 11)"
                    },
                    "tx_id": {
                        "type": "integer",
                        "description": "Optional CAN Tx arbitration ID override"
                    },
                    "rx_id": {
                        "type": "integer",
                        "description": "Optional CAN Rx arbitration ID override"
                    }
                }
            }
        }
    ])
}
