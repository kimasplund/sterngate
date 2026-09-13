#!/usr/bin/env python3
"""
Sterngate Vehicle Profile Localization Utility
Normalizes parameter and module names across all vehicle profile JSON files into:
1. Clean, unpolluted English `name`
2. Strongly typed `names` dictionary with en, de, sv translations
"""

import json
import glob
import os

PARAM_TRANSLATIONS = {
    "trans_fluid_temp": {
        "en": "Transmission Fluid Temperature",
        "de": "Getriebeöltemperatur",
        "sv": "Transmissionsoljetemperatur"
    },
    "tcc_slip_rpm": {
        "en": "Torque Converter Clutch Slip",
        "de": "Drehzahldifferenz KÜB",
        "sv": "Momentomvandlarkoppling slirning"
    },
    "turbine_speed": {
        "en": "Turbine Speed n2",
        "de": "Turbinendrehzahl n2",
        "sv": "Turbinvarvtal n2"
    },
    "output_speed": {
        "en": "Output Shaft Speed n3",
        "de": "Abtriebsdrehzahl n3",
        "sv": "Utgående axelvarvtal n3"
    },
    "engine_rpm": {
        "en": "Engine Speed",
        "de": "Motordrehzahl",
        "sv": "Motorvarvtal"
    },
    "coolant_temp": {
        "en": "Engine Coolant Temperature",
        "de": "Kühlmitteltemperatur",
        "sv": "Kylarvätsketemperatur"
    },
    "rail_pressure": {
        "en": "Common Rail Fuel Pressure",
        "de": "Raildruck Istwert",
        "sv": "Common Rail bränsletryck"
    },
    "boost_pressure": {
        "en": "Boost Pressure",
        "de": "Ladedruck Istwert",
        "sv": "Laddtryck ärvärde"
    },
    "air_mass": {
        "en": "Air Mass Flow",
        "de": "Luftmasse Soll/Ist",
        "sv": "Luftmassflöde"
    },
    "mass_air_flow": {
        "en": "Mass Air Flow (MAF)",
        "de": "Luftmassenmesser (LMM)",
        "sv": "Luftmassemätare (LMM)"
    },
    "throttle_angle": {
        "en": "Throttle Valve Angle",
        "de": "Drosselklappenwinkel",
        "sv": "Gasspjällsvinkel"
    },
    "lambda_b1s1": {
        "en": "Lambda Upstream O2 Voltage Bank 1",
        "de": "Lambdaspannung Bank 1 vor Kat",
        "sv": "Lambdaspänning bank 1 före katalysator"
    },
    "lambda_b2s1": {
        "en": "Lambda Upstream O2 Voltage Bank 2",
        "de": "Lambdaspannung Bank 2 vor Kat",
        "sv": "Lambdaspänning bank 2 före katalysator"
    },
    "inj_corr_cyl1": {
        "en": "Smooth Running Correction Cyl 1",
        "de": "Laufruheregler Zylinder 1",
        "sv": "Gångjämnhetsreglering cylinder 1"
    },
    "inj_corr_cyl2": {
        "en": "Smooth Running Correction Cyl 2",
        "de": "Laufruheregler Zylinder 2",
        "sv": "Gångjämnhetsreglering cylinder 2"
    },
    "inj_corr_cyl3": {
        "en": "Smooth Running Correction Cyl 3",
        "de": "Laufruheregler Zylinder 3",
        "sv": "Gångjämnhetsreglering cylinder 3"
    },
    "inj_corr_cyl4": {
        "en": "Smooth Running Correction Cyl 4",
        "de": "Laufruheregler Zylinder 4",
        "sv": "Gångjämnhetsreglering cylinder 4"
    },
    "inj_corr_cyl5": {
        "en": "Smooth Running Correction Cyl 5",
        "de": "Laufruheregler Zylinder 5",
        "sv": "Gångjämnhetsreglering cylinder 5"
    },
    "inj_corr_cyl6": {
        "en": "Smooth Running Correction Cyl 6",
        "de": "Laufruheregler Zylinder 6",
        "sv": "Gångjämnhetsreglering cylinder 6"
    },
    "sbc_accumulator_pressure": {
        "en": "SBC High Pressure Accumulator",
        "de": "Druck Hochdruckspeicher (SBC)",
        "sv": "SBC högtrycksackumulator"
    },
    "sbc_brake_press_fl": {
        "en": "Brake Pressure Front Left",
        "de": "Bremsdruck vorne links",
        "sv": "Bromstryck vänster fram"
    },
    "sbc_brake_press_fr": {
        "en": "Brake Pressure Front Right",
        "de": "Bremsdruck vorne rechts",
        "sv": "Bromstryck höger fram"
    },
    "sbc_brake_press_rl": {
        "en": "Brake Pressure Rear Left",
        "de": "Bremsdruck hinten links",
        "sv": "Bromstryck vänster bak"
    },
    "sbc_brake_press_rr": {
        "en": "Brake Pressure Rear Right",
        "de": "Bremsdruck hinten rechts",
        "sv": "Bromstryck höger bak"
    },
    "sbc_actuation_count": {
        "en": "Brake Pedal Actuation Cycles",
        "de": "Bremsbetätigungszähler (SBC)",
        "sv": "Bromspedal aktiveringsräknare (SBC)"
    },
    "battery_voltage_cgw": {
        "en": "Battery Supply Voltage Terminal 30",
        "de": "Batteriespannung Klemme 30",
        "sv": "Batterispänning Klämma 30"
    },
    "terminal_15_status": {
        "en": "Ignition State Terminal 15",
        "de": "Zündungsstatus Klemme 15",
        "sv": "Tändningsstatus klämma 15"
    },
    "dsg_clutch_temp": {
        "en": "DSG Dual Clutch Oil Temperature",
        "de": "DSG Doppelkupplung Öltemperatur",
        "sv": "DSG dubbelkopplingsoljetemperatur"
    },
    "zf_oil_temp": {
        "en": "ZF Transmission Sump Temperature",
        "de": "ZF Getriebeöltemperatur",
        "sv": "ZF växellådsoljetemperatur"
    }
}

MODULE_TRANSLATIONS = {
    "EDC16": {
        "en": "Engine Control Unit (Bosch EDC16)",
        "de": "Motorsteuergerät (Bosch EDC16)",
        "sv": "Motorstyrenhet (Bosch EDC16)"
    },
    "EDC17": {
        "en": "Engine Control Unit (Bosch EDC17)",
        "de": "Motorsteuergerät (Bosch EDC17)",
        "sv": "Motorstyrenhet (Bosch EDC17)"
    },
    "CR3": {
        "en": "Engine Control Unit (OM646 CDI 3)",
        "de": "Motorsteuergerät (OM646 CDI 3)",
        "sv": "Motorstyrenhet (OM646 CDI 3)"
    },
    "CR4": {
        "en": "Engine Control Unit (OM642 CDI 4)",
        "de": "Motorsteuergerät (OM642 CDI 4)",
        "sv": "Motorstyrenhet (OM642 CDI 4)"
    },
    "CR6EU5": {
        "en": "Engine Control Unit (OM642 CDI 6 EU5)",
        "de": "Motorsteuergerät (OM642 CDI 6 EU5)",
        "sv": "Motorstyrenhet (OM642 CDI 6 EU5)"
    },
    "CRD2": {
        "en": "Engine Control Unit (Delphi CRD2 / OM651)",
        "de": "Motorsteuergerät (Delphi CRD2 / OM651)",
        "sv": "Motorstyrenhet (Delphi CRD2 / OM651)"
    },
    "ME28": {
        "en": "Engine Control Unit (Bosch ME 2.8 Gasoline)",
        "de": "Motorsteuergerät (Bosch ME 2.8 Benzin)",
        "sv": "Motorstyrenhet (Bosch ME 2.8 bensin)"
    },
    "ME97": {
        "en": "Engine Control Unit (Bosch ME 9.7 Gasoline)",
        "de": "Motorsteuergerät (Bosch ME 9.7 Benzin)",
        "sv": "Motorstyrenhet (Bosch ME 9.7 bensin)"
    },
    "DDE6": {
        "en": "Digital Diesel Electronics (BMW DDE6 / M57)",
        "de": "Digitale Dieselelektronik (BMW DDE6 / M57)",
        "sv": "Digital dieselelektronik (BMW DDE6 / M57)"
    },
    "EGS52": {
        "en": "Electronic Transmission Control (722.6 / NAG1)",
        "de": "Elektronische Getriebesteuerung (722.6 / NAG1)",
        "sv": "Elektronisk transmissionsstyrning (722.6 / NAG1)"
    },
    "EGS53": {
        "en": "Electronic Transmission Control (722.6 Updated)",
        "de": "Elektronische Getriebesteuerung (722.6 Aktualisiert)",
        "sv": "Elektronisk transmissionsstyrning (722.6 uppdaterad)"
    },
    "VGSNAG2": {
        "en": "Fully Integrated Transmission Control (722.9 7G-Tronic)",
        "de": "Vollintegrierte Getriebesteuerung (722.9 7G-Tronic)",
        "sv": "Helintegrerad transmissionsstyrning (722.9 7G-Tronic)"
    },
    "EGS_6HP": {
        "en": "ZF 6HP Transmission Control",
        "de": "ZF 6HP Getriebesteuerung",
        "sv": "ZF 6HP transmissionsstyrning"
    },
    "DSG_DQ250": {
        "en": "Direct Shift Gearbox 6-Speed (DQ250)",
        "de": "Direktschaltgetriebe 6-Gang (DQ250)",
        "sv": "Direktväxellåda 6-växlad (DQ250)"
    },
    "SBC211": {
        "en": "Sensotronic Brake Control (SBC)",
        "de": "Sensotronic-Bremsregelung (SBC)",
        "sv": "Sensotronic bromsreglering (SBC)"
    },
    "ZGW211": {
        "en": "Central Gateway (ZGW N93)",
        "de": "Zentrales Gateway (ZGW N93)",
        "sv": "Central gateway (ZGW N93)"
    }
}

def localize_profile(path):
    with open(path, "r", encoding="utf-8") as fp:
        data = json.load(fp)

    # Localize modules
    for mod_id, mod in data.get("modules", {}).items():
        if mod_id in MODULE_TRANSLATIONS:
            trans = MODULE_TRANSLATIONS[mod_id]
            mod["name"] = trans["en"]
            mod["names"] = trans

    # Localize parameters
    for p in data.get("parameters", []):
        pid = p.get("id")
        if pid in PARAM_TRANSLATIONS:
            trans = PARAM_TRANSLATIONS[pid]
            p["name"] = trans["en"]
            p["names"] = trans

    with open(path, "w", encoding="utf-8") as fp:
        json.dump(data, fp, indent=2, ensure_ascii=False)
        fp.write("\n")

def main():
    pattern = "profiles/**/*.json"
    files = sorted(glob.glob(pattern, recursive=True))
    count = 0
    for f in files:
        if "schema" in f:
            continue
        localize_profile(f)
        count += 1
        print(f"Localizing {f} -> OK")
    print(f"Successfully localized {count} vehicle profile JSON files.")

if __name__ == "__main__":
    main()
