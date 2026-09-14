#!/usr/bin/env python3
"""
scripts/extract_extended_xentry_knowledge.py

Extracts and compiles:
1. 866 Workshop Actuator & Service Routines (0x31 RoutineControl) -> data/routine_catalog_mb.json
2. 3,155 Variant Coding Parameters (0x2E WriteDataByIdentifier) -> data/coding_catalog_mb.json
3. SecurityAccess (0x27) Seed-Key levels & routine counts -> enriches data/ecu_catalog.json
4. Sensor scaling and physical units from the 237 .sim files
"""

import os
import glob
import json
import re
from collections import defaultdict

SIM_DIR = "reference/xentry/extracted_diagnostics/simulations"
ROUTINE_OUT = "data/routine_catalog_mb.json"
CODING_OUT = "data/coding_catalog_mb.json"
ECU_CATALOG_FILE = "data/ecu_catalog.json"

ROUTINE_TRANSLATIONS = {
    "0xFF01": {
        "en": "Check Compatibility & Programming Dependencies",
        "de": "Kompatibilitäts- und Programmierabhängigkeiten prüfen",
        "sv": "Kontrollera kompatibilitets- och programmeringsberoenden",
        "category": "Flashing"
    },
    "0x0203": {
        "en": "Check Reprogramming Preconditions",
        "de": "Umprogrammierungs-Vorbedingungen prüfen",
        "sv": "Kontrollera omprogrammeringsförutsättningar",
        "category": "Flashing"
    },
    "0x0245": {
        "en": "Synchronize Non-Volatile Memory",
        "de": "Mit nichtflüchtigem Speicher synchronisieren",
        "sv": "Synkronisera till icke-flyktigt minne",
        "category": "System"
    },
    "0x0219": {
        "en": "ECU Input/Output Hardware Self-Test",
        "de": "Steuergeräte-E/A-Hardware-Selbsttest",
        "sv": "Styrenhet I/O-hårdvarutest",
        "category": "Diagnostics"
    },
    "0x0212": {
        "en": "Reset VIN & Vehicle Identification Values",
        "de": "Fahrgestellnummer-Werte zurücksetzen",
        "sv": "Återställ chassinummer (VIN)",
        "category": "Adaptation"
    },
    "0x0266": {
        "en": "Synchronize Secured System Date and Time",
        "de": "Gesicherte Systemzeit und Datum synchronisieren",
        "sv": "Synkronisera säker systemtid och datum",
        "category": "Security"
    },
    "0x0265": {
        "en": "Activate Debugging & Analysis Interfaces",
        "de": "Debugging-Schnittstellen aktivieren",
        "sv": "Aktivera felsökningsgränssnitt",
        "category": "Security"
    },
    "0x0207": {
        "en": "Pre-Check Programming Dependencies",
        "de": "Vorprüfung der Programmierabhängigkeiten",
        "sv": "Förkontroll av programmeringsberoenden",
        "category": "Flashing"
    },
    "0x0264": {
        "en": "Replace Security & Authorization Certificates",
        "de": "Sicherheitszertifikate austauschen",
        "sv": "Ersätt säkerhetscertifikat",
        "category": "Security"
    },
    "0x0267": {
        "en": "Security Certificate Self-Check",
        "de": "Zertifikats-Selbstprüfung",
        "sv": "Självtest av certifikat",
        "category": "Security"
    },
    "0x0246": {
        "en": "Erase Secondary Memory Controller Event DTCs",
        "de": "Ereignisspeicher sekundärer Speichercontroller löschen",
        "sv": "Radera sekundär felminneslogg",
        "category": "Diagnostics"
    },
    "0x0211": {
        "en": "Clear Resource Consumption & Operating Statistics",
        "de": "Ressourcenverbrauchsdaten löschen",
        "sv": "Rensa resursförbrukningsdata",
        "category": "Maintenance"
    },
    "0x027B": {
        "en": "Trust Model Cryptographic Key Pair Generation",
        "de": "Trust-Model Schlüsselpaar generieren",
        "sv": "Generera nyckelpar för Trust-modell",
        "category": "Security"
    },
    "0x0201": {
        "en": "EVC Protected ECU System Reset",
        "de": "EVC-geschützter Steuergeräte-Neustart",
        "sv": "EVC-skyddad styrdonsomstart",
        "category": "System"
    },
    "0x0240": {
        "en": "Clear Diagnostic Development & Logging Stack",
        "de": "Protokolldaten löschen",
        "sv": "Rensa loggdata",
        "category": "Diagnostics"
    },
    "0x0305": {
        "en": "Steering Angle Sensor Zero Position Calibration",
        "de": "Lenkwinkelsensor auf Nullposition kalibrieren",
        "sv": "Kalibrera styrvinkelsensor till nolläge",
        "category": "Chassis"
    },
    "0x0306": {
        "en": "Setting to Zero Point Calibration",
        "de": "Nullpunkt-Kalibrierung",
        "sv": "Nollpunktskalibrering",
        "category": "Chassis"
    },
    "0x0307": {
        "en": "Reset Learnt Steering Rack Length & Center Offset",
        "de": "Lenkgetriebe-Lernwerte und Nullpunktversatz zurücksetzen",
        "sv": "Återställ inlärda styrväxelvärden och nollpunktsförskjutning",
        "category": "Chassis"
    },
    "0x0312": {
        "en": "ILS Headlamp Alignment Mode & Dyno Test Bench Mode",
        "de": "ILS-Scheinwerfer-Einstellmodus & Rollenprüfstandsmodus aktivieren",
        "sv": "Aktivera ILS-strålkastarinställning & rulltestbänksläge",
        "category": "Calibration"
    },
    "0x0301": {
        "en": "Reset Cooling Fan Operating Hours & Battery Age Counter",
        "de": "HLI-Lüfter-Betriebsstundenzähler & Batterie-Alterungszähler zurücksetzen",
        "sv": "Återställ drifttid för kylfläkt & batteriets åldersräknare",
        "category": "Maintenance"
    },
    "0x0242": {
        "en": "Erase SAR Crash & Event Memory",
        "de": "SAR-Unfalldatenspeicher löschen",
        "sv": "Radera SAR-olycksdataminne",
        "category": "Safety"
    },
    "0x0313": {
        "en": "Deactivate Powertrain Torque Control Mode",
        "de": "Antriebsstrang-Drehmomentregelungsmodus deaktivieren",
        "sv": "Avaktivera drivlinans momentregleringsläge",
        "category": "Powertrain"
    },
    "0x0315": {
        "en": "Reset Limp-Home Entry Counter",
        "de": "Notlauf-Eintragszähler zurücksetzen",
        "sv": "Återställ nödkörningsräknare",
        "category": "Powertrain"
    },
    "0x0204": {
        "en": "SBC Pad Replacement Deactivation (0 Bar Hydraulic Safe Mode)",
        "de": "SBC-Bremsbelagwechsel Deaktivierung (0 bar Drucklos)",
        "sv": "SBC Bromsbeläggsbyte Avaktivering (0 bar trycklös)",
        "category": "Brakes"
    },
    "0x0206": {
        "en": "SBC System Reactivation & Pressure Charge (~160 Bar)",
        "de": "SBC-Systemreaktivierung & Druckaufbau (~160 bar)",
        "sv": "SBC Systemåteraktivering & Trycksättning (~160 bar)",
        "category": "Brakes"
    },
    "0x0218": {
        "en": "AdBlue / SCR Induction Lockout Warning Reset",
        "de": "AdBlue/SCR-Startverhinderung Warnungs-Rückstellung",
        "sv": "AdBlue/SCR Startspärr Varningsåterställning",
        "category": "Powertrain"
    },
    "0x0220": {
        "en": "ABC Hydraulic Surge Limiter (120 Bar Fallback Safe Mode)",
        "de": "ABC-Hydraulikdruckbegrenzer (120 bar Notlauf)",
        "sv": "ABC Hydrauliskt Tryckskydd (120 bar nödläge)",
        "category": "Suspension"
    },
    "0x0221": {
        "en": "ABC Strut Isolation Valve Lockout",
        "de": "ABC-Federbein-Sperrventile Verriegelung",
        "sv": "ABC Fjäderbensspärrventiler Låsning",
        "category": "Suspension"
    },
    "0xFF00": {
        "en": "Erase Flash Memory Sector Routine",
        "de": "Flash-Speichersektor löschen",
        "sv": "Radera flashminnessektor",
        "category": "Flashing"
    }
}

def clean_name(n):
    if not n:
        return ""
    n = (
        n.replace("_Start", "")
        .replace("_Stop", "")
        .replace("_Request", "")
        .replace("_Write", "")
        .replace("_Read", "")
    )
    n = re.sub(r"^[A-Za-z0-9_]{3,30}:\s*", "", n)
    n = n.replace("_", " ").strip()
    n = re.sub(r"\b(Write|Read)\b\s*$", "", n, flags=re.IGNORECASE).strip()
    return n

def main():
    print("[*] Starting Extended XENTRY Knowledge Ingestion...")
    os.makedirs("data", exist_ok=True)

    xml_files = glob.glob(os.path.join(SIM_DIR, "*_simulation.xml"))
    sim_files = glob.glob(os.path.join(SIM_DIR, "*.sim"))

    print(f"[*] Found {len(xml_files)} Simulation XML files and {len(sim_files)} .sim models.")

    routines = {} # routine_id -> dict
    coding_dids = {} # did_hex -> dict
    ecu_sec_levels = defaultdict(set) # ecu_name -> set of sec levels
    ecu_routines = defaultdict(set)
    ecu_codings = defaultdict(set)

    # 1. Parse Simulation XMLs for 0x31, 0x2E, 0x27
    print("[*] Parsing 0x31 Routines, 0x2E Coding DIDs, and 0x27 Security Access Levels from XMLs...")
    for fpath in xml_files:
        try:
            with open(fpath, "r", encoding="latin1") as fp:
                content = fp.read()
            
            ecu_m = re.search(r'<Info name=\"DIOName\">([^<]+)</Info>', content)
            ecu_name = ecu_m.group(1).strip().upper() if ecu_m else "UNKNOWN"

            # 0x31 RoutineControl
            # Matches: <!-- Name --> <Request len="...">0x3101XXXX...</Request>
            r_matches = re.findall(r'<!--\s*(.+?)\s*-->\s*<Request[^>]*>(0x3101[0-9A-Fa-f]{4})', content)
            for comment, req in r_matches:
                r_id = "0x" + req[6:].upper()
                c_name = clean_name(comment)
                ecu_routines[ecu_name].add(r_id)

                if r_id not in routines:
                    meta = ROUTINE_TRANSLATIONS.get(r_id, {})
                    routines[r_id] = {
                        "routine_id": r_id,
                        "name_en": meta.get("en", c_name),
                        "name_de": meta.get("de", c_name),
                        "name_sv": meta.get("sv", c_name),
                        "category": meta.get("category", "General Workshop"),
                        "raw_request_prefix": "0x3101" + r_id[2:],
                        "ecus": [ecu_name]
                    }
                else:
                    if ecu_name not in routines[r_id]["ecus"]:
                        routines[r_id]["ecus"].append(ecu_name)

            # 0x2E WriteDataByIdentifier (Variant Coding)
            # Matches: <!-- Name --> <Request len="N">0x2EXXXX...</Request>
            w_matches = re.findall(r'<!--\s*(.+?)\s*-->\s*<Request len=\"(\d+)\"[^>]*>(0x2E[0-9A-Fa-f]{4})', content)
            for comment, rlen, req in w_matches:
                did_hex = "0x" + req[4:].upper()
                c_name = clean_name(comment)
                ecu_codings[ecu_name].add(did_hex)

                data_len = max(0, int(rlen) - 3)
                is_vin = did_hex in ["0xF190", "0xF1A0", "0xF18A"] or "vin" in c_name.lower()
                is_fingerprint = did_hex in ["0xF15A", "0xF15B"] or "fingerprint" in c_name.lower()

                if did_hex not in coding_dids:
                    coding_dids[did_hex] = {
                        "did": did_hex,
                        "name": c_name,
                        "length_bytes": data_len,
                        "is_vin_parameter": is_vin,
                        "is_fingerprint": is_fingerprint,
                        "ecus": [ecu_name]
                    }
                else:
                    if ecu_name not in coding_dids[did_hex]["ecus"]:
                        coding_dids[did_hex]["ecus"].append(ecu_name)
                    if data_len > 0 and coding_dids[did_hex]["length_bytes"] == 0:
                        coding_dids[did_hex]["length_bytes"] = data_len

            # 0x27 SecurityAccess Levels
            sec_matches = re.findall(r'<Request[^>]*>(0x27[0-9A-Fa-f]{2})', content)
            for req in sec_matches:
                lvl = "0x" + req[4:].upper()
                # Store odd levels (Seed request)
                lvl_val = int(lvl, 16)
                if lvl_val % 2 == 1:
                    ecu_sec_levels[ecu_name].add(lvl)

        except Exception:
            pass

    # 2. Ingest 237 .sim files for sensor scaling, descriptions, and units
    print("[*] Ingesting sensor telemetry definitions from .sim files...")
    sim_sensors = {}
    for spath in sim_files:
        try:
            with open(spath, "r", encoding="latin1") as fp:
                stext = fp.read()
            
            # Find lines like: DT_25_BPSCD_pFltVal = VALID,T_FLOAT,12.3456,hPa,@Ladedruck P2@
            sensor_matches = re.findall(r'(DT_[A-Za-z0-9_]+)\s*=\s*[A-Z_]+,[A-Z_]+,[0-9.]+,\s*([^,]*)\s*,\s*@([^@]*)@', stext)
            for var_name, unit, desc in sensor_matches:
                unit = unit.strip()
                desc = desc.strip()
                if var_name not in sim_sensors and desc:
                    sim_sensors[var_name] = {
                        "name": var_name,
                        "unit": unit,
                        "description_de": desc
                    }
        except Exception:
            pass
    print(f"[+] Ingested {len(sim_sensors)} distinct factory telemetry variables with units.")

    # 3. Sort and save data/routine_catalog_mb.json
    sorted_routines = dict(sorted(routines.items(), key=lambda x: len(x[1]["ecus"]), reverse=True))
    routine_payload = {
        "metadata": {
            "title": "Mercedes-Benz Factory Workshop Service Routine Catalog (0x31)",
            "total_routines": len(sorted_routines),
            "version": "1.0"
        },
        "routines": sorted_routines
    }
    with open(ROUTINE_OUT, "w", encoding="utf-8") as fp:
        json.dump(routine_payload, fp, indent=2, ensure_ascii=False)
    print(f"[+] Saved {ROUTINE_OUT} ({len(sorted_routines)} routines, {os.path.getsize(ROUTINE_OUT) // 1024} KB)")

    # 4. Sort and save data/coding_catalog_mb.json
    sorted_codings = dict(sorted(coding_dids.items(), key=lambda x: len(x[1]["ecus"]), reverse=True))
    coding_payload = {
        "metadata": {
            "title": "Mercedes-Benz Variant Coding DID Catalog (0x2E)",
            "total_dids": len(sorted_codings),
            "version": "1.0"
        },
        "coding_dids": sorted_codings
    }
    with open(CODING_OUT, "w", encoding="utf-8") as fp:
        json.dump(coding_payload, fp, indent=2, ensure_ascii=False)
    print(f"[+] Saved {CODING_OUT} ({len(sorted_codings)} variant coding DIDs, {os.path.getsize(CODING_OUT) // 1024} KB)")

    # 5. Enrich data/ecu_catalog.json with Security Access levels, routines count, and coding DIDs count
    if os.path.exists(ECU_CATALOG_FILE):
        with open(ECU_CATALOG_FILE, "r", encoding="utf-8") as fp:
            cat_data = json.load(fp)
        
        ecus = cat_data.get("ecus", {})
        enriched_count = 0
        for name, entry in ecus.items():
            name_u = name.upper()
            sec_lvls = sorted(list(ecu_sec_levels.get(name_u, set())))
            if sec_lvls:
                # Prefer 0x11 (UDS programming) or 0x0B (variant coding) or first
                pref = "0x11" if "0x11" in sec_lvls else ("0x0B" if "0x0B" in sec_lvls else sec_lvls[0])
                entry["security_level"] = pref
                entry["supported_security_levels"] = sec_lvls
                enriched_count += 1
            
            entry["routines_count"] = len(ecu_routines.get(name_u, set()))
            entry["coding_dids_count"] = len(ecu_codings.get(name_u, set()))

        cat_data["metadata"]["version"] = "2.2"
        cat_data["metadata"]["description"] = f"Lean canonical diagnostic routing index for {len(ecus)} automotive electronic control units with security access levels, workshop routines, and coding DIDs."

        with open(ECU_CATALOG_FILE, "w", encoding="utf-8") as fp:
            json.dump(cat_data, fp, indent=2, ensure_ascii=False)
        print(f"[+] Updated {ECU_CATALOG_FILE}: Enriched {enriched_count} ECUs with SecurityAccess levels and routine/coding metrics.")

    print("\n[*] Extraction and compilation completed successfully!")

if __name__ == "__main__":
    main()
