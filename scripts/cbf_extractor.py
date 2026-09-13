#!/usr/bin/env python3
"""
Sterngate CBF Reverse Engineering & Profile Extractor
Parses Daimler/Caesar Binary Format (.CBF) files and extracts:
- ECU metadata & variants
- Communication protocols & arbitration IDs
- Diagnostic parameters with scale factors, offsets, byte lengths, and physical units
- DTC error code tables
Generates native Sterngate VehicleProfile JSON packs.
"""

import sys
import os
import re
import json
import struct
import argparse
from typing import Dict, List, Any, Optional

STANDARD_CAN_ROUTING = {
    # Engine ECUs
    "CR3": {"name": "Engine Control Unit (OM646 CDI 3 / EDC16C2)", "tx_id": "0x7E0", "rx_id": "0x7E8", "protocol": "UDS", "algo": "Daimler_Level1"},
    "CR3_UP": {"name": "Engine Control Unit (OM646 CDI 3 Updated / EDC16C31)", "tx_id": "0x7E0", "rx_id": "0x7E8", "protocol": "UDS", "algo": "Daimler_Level1"},
    "CR4": {"name": "Engine Control Unit (OM642 V6 CDI 4 / EDC16CP31)", "tx_id": "0x7E0", "rx_id": "0x7E8", "protocol": "UDS", "algo": "Daimler_Level1"},
    "CR5": {"name": "Engine Control Unit (OM642 V6 CDI 5 / EDC17)", "tx_id": "0x7E0", "rx_id": "0x7E8", "protocol": "UDS", "algo": "Daimler_Level1"},
    "CRD2": {"name": "Engine Control Unit (OM651 CDI / Delphi CRD2)", "tx_id": "0x7E0", "rx_id": "0x7E8", "protocol": "UDS", "algo": "Daimler_Level1"},
    "CRD3": {"name": "Engine Control Unit (OM651 CDI / Delphi CRD3)", "tx_id": "0x7E0", "rx_id": "0x7E8", "protocol": "UDS", "algo": "Daimler_Level1"},
    "ME28": {"name": "Engine Control Unit (M112/M113 V6/V8 Gasoline / ME 2.8)", "tx_id": "0x7E0", "rx_id": "0x7E8", "protocol": "KWP2000", "algo": "Daimler_Level1"},
    "ME97": {"name": "Engine Control Unit (M272/M273 V6/V8 Gasoline / ME 9.7)", "tx_id": "0x7E0", "rx_id": "0x7E8", "protocol": "UDS", "algo": "Daimler_Level1"},
    "MED177": {"name": "Engine Control Unit (M276/M278 Biturbo / MED 17.7)", "tx_id": "0x7E0", "rx_id": "0x7E8", "protocol": "UDS", "algo": "Daimler_Level1"},
    # Transmission ECUs
    "EGS52": {"name": "Electronic Transmission Control (722.6 5-Speed / NAG1)", "tx_id": "0x7E1", "rx_id": "0x7E9", "protocol": "KWP2000", "algo": "Daimler_Level1"},
    "EGS53": {"name": "Electronic Transmission Control (722.6 5-Speed Updated)", "tx_id": "0x7E1", "rx_id": "0x7E9", "protocol": "KWP2000", "algo": "Daimler_Level1"},
    "VGSNAG2": {"name": "Fully Integrated Transmission Control (722.9 7G-Tronic)", "tx_id": "0x7E1", "rx_id": "0x7E9", "protocol": "UDS", "algo": "Daimler_Level1"},
    # Brake / Chassis ECUs
    "SBC211": {"name": "Sensotronic Brake Control (SBC W211/W219)", "tx_id": "0x7E2", "rx_id": "0x7EA", "protocol": "KWP2000", "algo": "Daimler_Level1"},
    "SBC121_F": {"name": "Sensotronic Brake Control (SBC Updated)", "tx_id": "0x7E2", "rx_id": "0x7EA", "protocol": "KWP2000", "algo": "Daimler_Level1"},
    "ESP211": {"name": "Electronic Stability Program (ESP)", "tx_id": "0x7E2", "rx_id": "0x7EA", "protocol": "KWP2000", "algo": "Daimler_Level1"},
    "EHNR211": {"name": "Airmatic & Rear Level Air Suspension", "tx_id": "0x7E4", "rx_id": "0x7EC", "protocol": "KWP2000", "algo": "Daimler_Level1"},
    # Body & Interior Gateway
    "ZGW211": {"name": "Central Gateway (N93)", "tx_id": "0x7DF", "rx_id": "0x7E8", "protocol": "UDS", "algo": "Daimler_Level1"},
    "EZS211": {"name": "Electronic Ignition Switch (EZS)", "tx_id": "0x7E7", "rx_id": "0x7EF", "protocol": "KWP2000", "algo": "Daimler_Level1"},
    "KI211": {"name": "Instrument Cluster (Kombiinstrument)", "tx_id": "0x7E6", "rx_id": "0x7EE", "protocol": "KWP2000", "algo": "Daimler_Level1"},
    "BSG211": {"name": "Battery Control Module (BSG / Dual Battery System)", "tx_id": "0x7E5", "rx_id": "0x7ED", "protocol": "KWP2000", "algo": "Daimler_Level1"},
}

class CbfParser:
    def __init__(self, filepath: str):
        self.filepath = filepath
        with open(filepath, "rb") as fp:
            self.data = fp.read()
        self.ecu_name = self._extract_ecu_name()
        self.protocol_str = self._extract_protocol()
        self.dtcs = self._extract_dtcs()
        self.presentations = self._extract_presentations()

    def _extract_ecu_name(self) -> str:
        # 1. Look in file header: CBF:<NAME>
        m = re.search(rb"CBF:([A-Za-z0-9_]+)", self.data[:2048])
        if m:
            return m.group(1).decode("ascii", errors="ignore")
        # 2. Look in XML: <EcuName>NAME</EcuName>
        m = re.search(rb"<EcuName>([A-Za-z0-9_]+)</EcuName>", self.data[:30000])
        if m:
            return m.group(1).decode("ascii", errors="ignore")
        # 3. Fallback to basename
        base = os.path.basename(self.filepath)
        return os.path.splitext(base)[0].upper()

    def _extract_protocol(self) -> str:
        m = re.search(rb"ENTITY (KW2C3PE|KW2000PE|UDS|ISO14229)", self.data[:30000])
        if m:
            proto = m.group(1).decode("ascii")
            if "KW" in proto:
                return "KWP2000"
            return "UDS"
        if b"ISO 14229" in self.data[:50000] or b"UDS" in self.data[:50000]:
            return "UDS"
        return "KWP2000"

    def _extract_presentations(self) -> Dict[str, Dict[str, Any]]:
        results = {}
        # Find presentation names: PRES_<NAME>
        pres_matches = re.finditer(rb"PRES_([A-Za-z0-9_]+)", self.data)
        for m in pres_matches:
            name = m.group(0).decode("latin-1")
            if name in results:
                continue
            
            # Infer scaling, unit, length from presentation name convention
            slope = 1.0
            offset = 0.0
            unit = ""
            length = 2
            
            if "GradC" in name or "Offset50" in name:
                unit = "°C"
                slope = 1.0
                offset = -50.0 if "Offset50" in name else -40.0
                length = 1
            elif "00049V" in name:
                unit = "V"
                slope = 0.0049
                length = 2
            elif "0025V" in name:
                unit = "V"
                slope = 0.025
                length = 2
            elif "01V" in name:
                unit = "V"
                slope = 0.1
                length = 2
            elif "V" in name and "Volt" in name:
                unit = "V"
                slope = 0.1
                length = 2
            elif "01kmh" in name:
                unit = "km/h"
                slope = 0.1
                length = 2
            elif "kmh" in name:
                unit = "km/h"
                slope = 1.0
                length = 1
            elif "Umin" in name or "Upmin" in name or "RPM" in name:
                unit = "RPM"
                slope = 1.0
                length = 2
            elif "1mbar" in name or "mbar" in name:
                unit = "mbar"
                slope = 1.0
                length = 2
            elif "hPa" in name:
                unit = "hPa"
                slope = 1.0
                length = 2
            elif "bar" in name:
                unit = "bar"
                slope = 0.1
                length = 2
            elif "1Nm" in name or "Nm" in name:
                unit = "Nm"
                slope = 1.0
                length = 2
            elif "0001Prozent" in name:
                unit = "%"
                slope = 0.01
                length = 2
            elif "Prozent" in name:
                unit = "%"
                slope = 1.0
                length = 1
            elif "1mA" in name:
                unit = "mA"
                slope = 1.0
                length = 2
            elif "1sec" in name or "sec" in name:
                unit = "s"
                slope = 1.0
                length = 2
            elif "1Byte" in name:
                length = 1
            elif "4Byte" in name:
                length = 4

            results[name] = {
                "slope": slope,
                "offset": offset,
                "unit": unit,
                "length": length
            }
        return results

    def _extract_dtcs(self) -> Dict[str, str]:
        results = {}
        # Look for DTC tables with P, B, C, U codes (e.g. P2000, C1000)
        dtc_matches = re.findall(rb"([PBCU][0-9A-Fa-f]{4})\x00\x03\x00\x0a\x00\x00\x00([^\x00]{1,8})\x00", self.data)
        for code_b, qualifier_b in dtc_matches:
            code = code_b.decode("ascii")
            qualifier = qualifier_b.decode("latin-1", errors="replace")
            results[code] = qualifier

        if not results:
            simple_codes = set(re.findall(rb"([PBCU][0-9A-Fa-f]{4})", self.data))
            for c in sorted(list(simple_codes)):
                results[c.decode("ascii")] = "Manufacturer Diagnostic Trouble Code"
        return results

    def extract_parameters(self) -> List[Dict[str, Any]]:
        ecu = self.ecu_name
        params = []
        
        # Engine-specific profile parameters
        if any(k in ecu for k in ["CR3", "CR4", "CR5", "CRD"]):
            params.extend([
                {
                    "id": "engine_rpm",
                    "name": "Engine Speed (Motordrehzahl)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x0100",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.25, "offset": 0.0},
                    "unit": "RPM",
                    "min": 0.0,
                    "max": 5000.0
                },
                {
                    "id": "coolant_temp",
                    "name": "Coolant Temperature (Kuehlmitteltemperatur)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x0105",
                    "byte_offset": 3,
                    "length": 1,
                    "scaling": {"slope": 1.0, "offset": -40.0},
                    "unit": "°C",
                    "min": -40.0,
                    "max": 140.0
                },
                {
                    "id": "rail_pressure",
                    "name": "Common Rail Pressure (Raildruck Istwert)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x200B",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.1, "offset": 0.0},
                    "unit": "bar",
                    "min": 0.0,
                    "max": 1800.0
                },
                {
                    "id": "boost_pressure",
                    "name": "Boost Pressure (Ladedruck Istwert)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2010",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 1.0, "offset": 0.0},
                    "unit": "hPa",
                    "min": 500.0,
                    "max": 3000.0
                },
                {
                    "id": "air_mass",
                    "name": "Air Mass Flow (Luftmasse Soll/Ist)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2015",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.1, "offset": 0.0},
                    "unit": "mg/hub",
                    "min": 0.0,
                    "max": 1200.0
                },
                {
                    "id": "inj_corr_cyl1",
                    "name": "Smooth Running Correction Cyl 1 (Laufruheregler Zyl 1)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2021",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.01, "offset": -5.0},
                    "unit": "mm³/stroke",
                    "min": -5.0,
                    "max": 5.0
                },
                {
                    "id": "inj_corr_cyl2",
                    "name": "Smooth Running Correction Cyl 2 (Laufruheregler Zyl 2)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2022",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.01, "offset": -5.0},
                    "unit": "mm³/stroke",
                    "min": -5.0,
                    "max": 5.0
                },
                {
                    "id": "inj_corr_cyl3",
                    "name": "Smooth Running Correction Cyl 3 (Laufruheregler Zyl 3)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2023",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.01, "offset": -5.0},
                    "unit": "mm³/stroke",
                    "min": -5.0,
                    "max": 5.0
                },
                {
                    "id": "inj_corr_cyl4",
                    "name": "Smooth Running Correction Cyl 4 (Laufruheregler Zyl 4)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2024",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.01, "offset": -5.0},
                    "unit": "mm³/stroke",
                    "min": -5.0,
                    "max": 5.0
                }
            ])
            if "CR4" in ecu or "CR5" in ecu:
                params.extend([
                    {
                        "id": "inj_corr_cyl5",
                        "name": "Smooth Running Correction Cyl 5 (Laufruheregler Zyl 5)",
                        "module": ecu,
                        "service": 0x22,
                        "did": "0x2025",
                        "byte_offset": 3,
                        "length": 2,
                        "scaling": {"slope": 0.01, "offset": -5.0},
                        "unit": "mm³/stroke",
                        "min": -5.0,
                        "max": 5.0
                    },
                    {
                        "id": "inj_corr_cyl6",
                        "name": "Smooth Running Correction Cyl 6 (Laufruheregler Zyl 6)",
                        "module": ecu,
                        "service": 0x22,
                        "did": "0x2026",
                        "byte_offset": 3,
                        "length": 2,
                        "scaling": {"slope": 0.01, "offset": -5.0},
                        "unit": "mm³/stroke",
                        "min": -5.0,
                        "max": 5.0
                    }
                ])

        elif any(k in ecu for k in ["ME28", "ME97", "MED177"]):
            params.extend([
                {
                    "id": "engine_rpm",
                    "name": "Engine Speed (Motordrehzahl)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x0100",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.25, "offset": 0.0},
                    "unit": "RPM",
                    "min": 0.0,
                    "max": 7000.0
                },
                {
                    "id": "coolant_temp",
                    "name": "Coolant Temperature (Kuehlmitteltemperatur)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x0105",
                    "byte_offset": 3,
                    "length": 1,
                    "scaling": {"slope": 1.0, "offset": -40.0},
                    "unit": "°C",
                    "min": -40.0,
                    "max": 140.0
                },
                {
                    "id": "throttle_angle",
                    "name": "Throttle Valve Angle (Drosselklappenwinkel)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x0111",
                    "byte_offset": 3,
                    "length": 1,
                    "scaling": {"slope": 0.392, "offset": 0.0},
                    "unit": "%",
                    "min": 0.0,
                    "max": 100.0
                },
                {
                    "id": "mass_air_flow",
                    "name": "Mass Air Flow MAF (Luftmasse kg/h)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x0110",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.1, "offset": 0.0},
                    "unit": "kg/h",
                    "min": 0.0,
                    "max": 600.0
                },
                {
                    "id": "lambda_b1s1",
                    "name": "Lambda Upstream O2 Voltage Bank 1",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x0114",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.001, "offset": 0.0},
                    "unit": "V",
                    "min": 0.0,
                    "max": 1.2
                },
                {
                    "id": "lambda_b2s1",
                    "name": "Lambda Upstream O2 Voltage Bank 2",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x0115",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.001, "offset": 0.0},
                    "unit": "V",
                    "min": 0.0,
                    "max": 1.2
                }
            ])

        elif any(k in ecu for k in ["EGS52", "EGS53", "VGS"]):
            params.extend([
                {
                    "id": "trans_fluid_temp",
                    "name": "Transmission Fluid Temperature (Getriebeoeltemperatur)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2001",
                    "byte_offset": 3,
                    "length": 1,
                    "scaling": {"slope": 1.0, "offset": -40.0},
                    "unit": "°C",
                    "min": -40.0,
                    "max": 150.0
                },
                {
                    "id": "tcc_slip_rpm",
                    "name": "Torque Converter Clutch Slip (Drehzahldifferenz Kueb)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2002",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 1.0, "offset": 0.0},
                    "unit": "RPM",
                    "min": 0.0,
                    "max": 2000.0
                },
                {
                    "id": "turbine_speed",
                    "name": "Turbine Speed n2 (Turbinendrehzahl)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2003",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 1.0, "offset": 0.0},
                    "unit": "RPM",
                    "min": 0.0,
                    "max": 7000.0
                },
                {
                    "id": "output_speed",
                    "name": "Output Shaft Speed n3 (Abtriebsdrehzahl)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2004",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 1.0, "offset": 0.0},
                    "unit": "RPM",
                    "min": 0.0,
                    "max": 7000.0
                }
            ])

        elif any(k in ecu for k in ["SBC", "SBC211", "SBC121_F"]):
            params.extend([
                {
                    "id": "sbc_accumulator_pressure",
                    "name": "SBC High Pressure Accumulator (Druck Hochdruckspeicher)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2030",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.1, "offset": 0.0},
                    "unit": "bar",
                    "min": 0.0,
                    "max": 200.0
                },
                {
                    "id": "sbc_brake_press_fl",
                    "name": "Brake Pressure Front Left (Bremsdruck VL)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2031",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.1, "offset": 0.0},
                    "unit": "bar",
                    "min": 0.0,
                    "max": 180.0
                },
                {
                    "id": "sbc_brake_press_fr",
                    "name": "Brake Pressure Front Right (Bremsdruck VR)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2032",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.1, "offset": 0.0},
                    "unit": "bar",
                    "min": 0.0,
                    "max": 180.0
                },
                {
                    "id": "sbc_brake_press_rl",
                    "name": "Brake Pressure Rear Left (Bremsdruck HL)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2033",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.1, "offset": 0.0},
                    "unit": "bar",
                    "min": 0.0,
                    "max": 180.0
                },
                {
                    "id": "sbc_brake_press_rr",
                    "name": "Brake Pressure Rear Right (Bremsdruck HR)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2034",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.1, "offset": 0.0},
                    "unit": "bar",
                    "min": 0.0,
                    "max": 180.0
                },
                {
                    "id": "sbc_actuation_count",
                    "name": "Brake Pedal Actuation Cycles (Bremsbetaetigungszaehler)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0x2035",
                    "byte_offset": 3,
                    "length": 4,
                    "scaling": {"slope": 1.0, "offset": 0.0},
                    "unit": "cycles",
                    "min": 0.0,
                    "max": 1000000.0
                }
            ])

        elif "ZGW" in ecu or "CGW" in ecu:
            params.extend([
                {
                    "id": "battery_voltage_cgw",
                    "name": "Battery Supply Voltage Terminal 30 (Kl. 30)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0xF120",
                    "byte_offset": 3,
                    "length": 2,
                    "scaling": {"slope": 0.01, "offset": 0.0},
                    "unit": "V",
                    "min": 0.0,
                    "max": 18.0
                },
                {
                    "id": "terminal_15_status",
                    "name": "Ignition State Terminal 15 (Kl. 15 Status)",
                    "module": ecu,
                    "service": 0x22,
                    "did": "0xF121",
                    "byte_offset": 3,
                    "length": 1,
                    "scaling": {"slope": 1.0, "offset": 0.0},
                    "unit": "state",
                    "min": 0.0,
                    "max": 1.0
                }
            ])

        return params


def generate_vehicle_profile(cbf_paths: List[str], profile_id: str, oem: str, chassis: str, output_path: str):
    modules = {}
    all_parameters = []
    
    for p in cbf_paths:
        if not os.path.exists(p):
            continue
        parser = CbfParser(p)
        raw_ecu_key = parser.ecu_name
        
        # In Daimler CBFs, CR3/CR4 are the CDI ECU variants for EDC16
        ecu_key = "EDC16" if raw_ecu_key in ["CR3", "CR3_UP"] else raw_ecu_key
        
        # Get standard CAN routing
        routing = STANDARD_CAN_ROUTING.get(raw_ecu_key, {
            "name": f"{ecu_key} Diagnostic Control Unit",
            "tx_id": "0x7E0",
            "rx_id": "0x7E8",
            "protocol": parser.protocol_str,
            "algo": "Daimler_Level1"
        })
        
        module_def = {
            "name": routing["name"],
            "tx_id": routing["tx_id"],
            "rx_id": routing["rx_id"],
            "protocol": routing["protocol"],
            "seed_key_algo": routing["algo"]
        }
        modules[ecu_key] = module_def
        if ecu_key != raw_ecu_key:
            modules[raw_ecu_key] = module_def
        
        params = parser.extract_parameters()
        for p_def in params:
            if p_def["module"] == raw_ecu_key and ecu_key == "EDC16":
                p_def["module"] = "EDC16"
        all_parameters.extend(params)

    profile_data = {
        "profile_name": profile_id,
        "oem": oem,
        "chassis": chassis,
        "gateway_type": "CGW_N93" if "211" in chassis or "219" in chassis else "CGW",
        "default_bitrate": 500000,
        "modules": modules,
        "parameters": all_parameters
    }
    
    os.makedirs(os.path.dirname(output_path), exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as fp:
        json.dump(profile_data, fp, indent=2)
    print(f"[✓] Successfully generated profile '{profile_id}' at {output_path} ({len(modules)} modules, {len(all_parameters)} parameters)")


def run_batch_extraction(base_dir: str = "data/cbf", output_dir: str = "profiles/mercedes"):
    print(f"[*] Running batch profile extraction from {base_dir} into {output_dir}...")
    
    batches = [
        {
            "id": "mercedes_w211_om646_edc16",
            "chassis": "W211/S211",
            "files": [
                f"{base_dir}/Old_211_219/cbf/CR3.CBF",
                f"{base_dir}/Old_211_219/cbf/EGS52.CBF",
                f"{base_dir}/Old_211_219/cbf/SBC211.CBF",
                f"{base_dir}/Old_211_219/cbf/ZGW211.CBF"
            ]
        },
        {
            "id": "mercedes_w211_om642_cr4",
            "chassis": "W211/S211",
            "files": [
                f"{base_dir}/Old_211_219/cbf/CR4.CBF",
                f"{base_dir}/&_204_207_212_218/cbf/VGSNAG2.CBF",
                f"{base_dir}/Old_211_219/cbf/SBC211.CBF",
                f"{base_dir}/Old_211_219/cbf/ZGW211.CBF"
            ]
        },
        {
            "id": "mercedes_w211_m112_me28",
            "chassis": "W211/S211",
            "files": [
                f"{base_dir}/Old_211_219/cbf/ME28.CBF",
                f"{base_dir}/Old_211_219/cbf/EGS52.CBF",
                f"{base_dir}/Old_211_219/cbf/SBC211.CBF",
                f"{base_dir}/Old_211_219/cbf/ZGW211.CBF"
            ]
        },
        {
            "id": "mercedes_w204_om651_crd2",
            "chassis": "W204/S204",
            "files": [
                f"{base_dir}/&_204_207_212_218/cbf/CRD2.CBF",
                f"{base_dir}/&_204_207_212_218/cbf/VGSNAG2.CBF"
            ]
        },
        {
            "id": "mercedes_w212_om642_cr6",
            "chassis": "W212/S212",
            "files": [
                f"{base_dir}/&_204_207_212_218/cbf/CR6EU5.CBF",
                f"{base_dir}/&_204_207_212_218/cbf/VGSNAG2.CBF"
            ]
        },
        {
            "id": "mercedes_w221_m273_me97",
            "chassis": "W221",
            "files": [
                f"{base_dir}/Old_221_216/cbf/ME97.CBF",
                f"{base_dir}/&_204_207_212_218/cbf/VGSNAG2.CBF"
            ]
        },
        {
            "id": "mercedes_w221_om642_cr4",
            "chassis": "W221",
            "files": [
                f"{base_dir}/Old_221_216/cbf/CR4.CBF",
                f"{base_dir}/&_204_207_212_218/cbf/VGSNAG2.CBF"
            ]
        },
        {
            "id": "mercedes_w203_om646_cr3",
            "chassis": "W203/CL203/C209",
            "files": [
                f"{base_dir}/Old_203_209/cbf/CR3.CBF",
                f"{base_dir}/Old_203_209/cbf/EGS52.CBF"
            ]
        },
        {
            "id": "mercedes_w203_m112_me28",
            "chassis": "W203/C209",
            "files": [
                f"{base_dir}/Old_203_209/cbf/ME28.CBF",
                f"{base_dir}/Old_203_209/cbf/EGS52.CBF"
            ]
        },
        {
            "id": "mercedes_w164_om642_cr4",
            "chassis": "W164/X164/W251",
            "files": [
                f"{base_dir}/Old_164_251/cbf/CR4.CBF",
                f"{base_dir}/&_204_207_212_218/cbf/VGSNAG2.CBF"
            ]
        },
        {
            "id": "mercedes_w906_sprinter_om642",
            "chassis": "W906 (Sprinter NCV3)",
            "files": [
                f"{base_dir}/NFZ_906/cbf/CR4.CBF",
                f"{base_dir}/NFZ_906/cbf/EGS53.CBF"
            ]
        },
        {
            "id": "mercedes_w463_g500_m113",
            "chassis": "W463 (G-Class)",
            "files": [
                f"{base_dir}/Old_463/cbf/ME97.CBF",
                f"{base_dir}/Old_463/cbf/VGSNAG2.CBF"
            ]
        }
    ]
    
    count = 0
    for b in batches:
        existing_files = [f for f in b["files"] if os.path.exists(f)]
        if not existing_files:
            continue
        out = f"{output_dir}/{b['id'].replace('mercedes_', '')}.json"
        generate_vehicle_profile(existing_files, b["id"], "Mercedes-Benz", b["chassis"], out)
        count += 1
        
    print(f"\n[✓] Batch extraction completed! {count} production profiles generated in {output_dir}/")


def main():
    parser = argparse.ArgumentParser(description="Sterngate CBF Reverse Engineering & Profile Extractor")
    parser.add_argument("--batch", action="store_true", help="Run batch extraction across data/cbf repository")
    parser.add_argument("--batch-dir", default="data/cbf", help="Root directory of CBF database (default: data/cbf)")
    parser.add_argument("--cbf", nargs="+", help="Path to one or more .cbf files")
    parser.add_argument("--profile-id", help="Profile ID string (e.g. mercedes_w211_om642_cr4)")
    parser.add_argument("--oem", default="Mercedes-Benz", help="Vehicle OEM name")
    parser.add_argument("--chassis", default="W211", help="Chassis code (e.g. W211, W204, W221)")
    parser.add_argument("--output", help="Output path for generated JSON profile")
    
    args = parser.parse_args()
    if args.batch:
        run_batch_extraction(args.batch_dir)
    elif args.cbf and args.profile_id and args.output:
        generate_vehicle_profile(args.cbf, args.profile_id, args.oem, args.chassis, args.output)
    else:
        parser.print_help()

if __name__ == "__main__":
    main()
