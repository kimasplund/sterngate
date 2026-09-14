#!/usr/bin/env python3
"""
scripts/compile_xentry_knowledge.py
Compiles extracted XENTRY simulation XMLs and CBF files into:
1. data/dtc_database_mb.json (OEM DTC dictionary in EN/DE)
2. data/ecu_catalog.json (Enriched canonical ECU index)
3. profiles/mercedes/*.json (Complete multi-ECU chassis profiles)
"""

import os
import glob
import json
import re
import xml.etree.ElementTree as ET
from collections import defaultdict

SIM_DIR = "reference/xentry/extracted_diagnostics/simulations"
CBF_DIR = "reference/xentry/extracted_diagnostics/cbf"
DTC_OUT = "data/dtc_database_mb.json"
ECU_CATALOG_FILE = "data/ecu_catalog.json"
PROFILES_OUT = "profiles/mercedes"

DID_TRANSLATIONS = {
    "Reprogramming Attempt Counter": {
        "de": "Umprogrammierungs-Versuchszähler",
        "sv": "Omprogrammeringsförsöksräknare"
    },
    "Diagnostic Trace Memory": {
        "de": "Diagnose-Ablaufverfolgungsspeicher",
        "sv": "Diagnostiskt spårningsminne"
    },
    "Reprogramming Resume Information": {
        "de": "Umprogrammierungs-Fortsetzungsinformation",
        "sv": "Omprogrammeringsåterupptagningsinformation"
    },
    "Vehicle Odometer in Low Resolution": {
        "de": "Fahrzeug-Kilometerstand (geringe Auflösung)",
        "sv": "Vägmätare (låg upplösning)"
    },
    "Usage Histogram": {
        "de": "Nutzungs-Histogramm",
        "sv": "Användningshistogram"
    },
    "Activate SAR Data Storage": {
        "de": "SAR-Datenspeicherung aktivieren",
        "sv": "Aktivera SAR-datalagring"
    },
    "Adjust ISO 15765 2 Block Size and STmin Parameter": {
        "de": "ISO 15765-2 Blockgröße und STmin anpassen",
        "sv": "Justera ISO 15765-2 blockstorlek och STmin"
    },
    "Adjust ISO 10681 2 Bandwidth Control Parameters": {
        "de": "ISO 10681-2 Bandbreitensteuerungsparameter anpassen",
        "sv": "Justera ISO 10681-2 bandbreddskontrollparametrar"
    },
    "SAR Trigger Counter": {
        "de": "SAR-Auslösezähler",
        "sv": "SAR-utlösarräknare"
    },
    "Number of SAR Write Cycles": {
        "de": "Anzahl der SAR-Schreibzyklen",
        "sv": "Antal SAR-skrivcykler"
    },
    "Global Time Sync Measured Values": {
        "de": "Globale Zeitsynchronisations-Messwerte",
        "sv": "Globala tidssynkroniseringsmätvärden"
    },
    "Used EVC Config": {
        "de": "Verwendete EVC-Konfiguration",
        "sv": "Använd EVC-konfiguration"
    },
    "Read Used EVC Config": {
        "de": "Verwendete EVC-Konfiguration lesen",
        "sv": "Läs använd EVC-konfiguration"
    },
    "Read Stored EVC Configuration": {
        "de": "Gespeicherte EVC-Konfiguration lesen",
        "sv": "Läs sparad EVC-konfiguration"
    }
}

def clean_text(t):
    if not t: return ""
    t = t.replace("\r", " ").replace("\n", " ").strip()
    # Strip internal engineering module prefixes like "ECM3_Mon3Vd: " or "NAG_18: "
    t = re.sub(r'^[A-Za-z0-9_]{3,50}:\s*', '', t)
    # Fix Daimler's broken umlauts from internal export
    t = t.replace("Funktionsst?rung", "Funktionsstörung")
    t = t.replace("au?erhalb", "außerhalb")
    t = t.replace("zul?ssig", "zulässig")
    t = t.replace("Plausibilit?t", "Plausibilität")
    t = t.replace("Verz?gerung", "Verzögerung")
    t = t.replace("Steuerger?t", "Steuergerät")
    t = t.replace("Betriebszust?nde", "Betriebszustände")
    t = t.replace("G?ltig", "Gültig")
    t = t.replace("unvollst?ndig", "unvollständig")
    t = t.replace("besch?digt", "beschädigt")
    # Clean redundant trailing characters
    t = t.rstrip(' _@')
    return t

def main():
    print("[*] Starting XENTRY Diagnostic Knowledge Ingestion...")
    os.makedirs("data", exist_ok=True)
    os.makedirs(PROFILES_OUT, exist_ok=True)

    dtc_dict = {} # code -> {"en": ..., "de": ..., "hex": ...}
    ecu_metadata = {} # ecu_name -> {tx_id, rx_id, protocol, doip_gw, doip_ecu, dtcs, dids, chassis_list}

    # 1. Parse Simulation XMLs
    sim_files = glob.glob(os.path.join(SIM_DIR, "*_simulation.xml"))
    print(f"[*] Parsing {len(sim_files)} ECU Simulation Models...")

    for fpath in sim_files:
        try:
            with open(fpath, "r", encoding="ISO-8859-1") as f:
                content = f.read()

            tree = ET.fromstring(content)
            header = tree.find("Header")
            if header is None: continue

            dio_name = None
            protocol = "UDS"
            variant = ""
            for info in header.findall("Info"):
                attr = info.get("name")
                if attr == "DIOName": dio_name = info.text
                elif attr == "Protocol" and info.text: protocol = info.text.strip()
                elif attr == "Variant" and info.text: variant = info.text.strip()

            if not dio_name: continue
            dio_name = dio_name.strip().upper()

            # CAN & DoIP parameters
            ecu_el = tree.find("ECUList/ECU")
            can_req, can_res, doip_gw, doip_ecu = None, None, None, None
            if ecu_el is not None:
                com = ecu_el.find("ComParameter")
                if com is not None:
                    cr = com.find("CanIdRequest")
                    cs = com.find("CanIdResponse")
                    dg = com.find("DoipGatewayAddress")
                    de = com.find("DoipEcuAddress")
                    if cr is not None and cr.text: can_req = cr.text.strip().lower()
                    if cs is not None and cs.text: can_res = cs.text.strip().lower()
                    if dg is not None and dg.text: doip_gw = dg.text.strip().lower()
                    if de is not None and de.text: doip_ecu = de.text.strip().lower()

            # Extract DTCs
            # Matches: <!-- P164456 (0x164456  0x00): An incorrect variant coding or configuration was detected. -->
            dtc_matches = re.findall(r'<!--\s*([PBUC0-9]{7})\s*\((0x[0-9A-Fa-f]+)\s*0x00\):\s*(.+?)\s*-->', content)
            ecu_dtcs = set()
            for code, hex_val, desc in dtc_matches:
                desc = clean_text(desc)
                ecu_dtcs.add(code)
                # Check language heuristic
                is_de = any(w in desc.lower() for w in ["hat funktionsstörung", "funktion", "kurzschluss", "masse", "steuergerät", "bauteil", "leitung", "unterbrechung", "plausibilität"])
                
                # Standard 5-char code alias (e.g. P1644 from P164456)
                std_code = code[:5]

                for c in [code, std_code]:
                    if c not in dtc_dict:
                        dtc_dict[c] = {"hex": hex_val}
                    if is_de:
                        dtc_dict[c]["de"] = desc
                    else:
                        dtc_dict[c]["en"] = desc

            # Extract DIDs
            # Matches: <!-- Name --> <Request len="3">0x220100</Request>
            did_matches = re.findall(r'<!--\s*([a-zA-Z0-9_ -]+?)\s*-->\s*<Request[^>]*>0x22([0-9A-Fa-f]{4})</Request>', content)
            ecu_dids = []
            seen_dids = set()
            for name, did_hex in did_matches:
                did_hex = "0x" + did_hex.upper()
                clean_n = clean_text(name).replace("_Read", "").replace("_", " ")
                if did_hex not in seen_dids:
                    seen_dids.add(did_hex)
                    ecu_dids.append({
                        "did": did_hex,
                        "name": clean_n,
                    })

            # Associate with chassis from filename / variant
            chassis = []
            f_lower = os.path.basename(fpath).lower()
            if "223" in f_lower or "223" in variant: chassis.append("W223")
            if "213" in f_lower or "213" in variant: chassis.append("W213")
            if "205" in f_lower or "205" in variant: chassis.append("W205")
            if "206" in f_lower or "206" in variant: chassis.append("W206")
            if "222" in f_lower or "222" in variant: chassis.append("W222")
            if "290" in f_lower or "290" in variant: chassis.append("W290_AMG")
            if "177" in f_lower or "177" in variant: chassis.append("W177")
            if "167" in f_lower or "167" in variant: chassis.append("W167")
            if "463" in f_lower or "464" in f_lower or "465" in f_lower: chassis.append("W463")
            if "907" in f_lower: chassis.append("Sprinter_907")
            if "447" in f_lower: chassis.append("VClass_447")
            if not chassis: chassis.append("Mercedes_Universal")

            ecu_metadata[dio_name] = {
                "name": dio_name,
                "protocol": protocol,
                "tx_id": can_req,
                "rx_id": can_res,
                "doip_gateway": doip_gw,
                "doip_ecu": doip_ecu,
                "dtc_count": len(ecu_dtcs),
                "dids": ecu_dids,
                "chassis": chassis,
            }

        except Exception as e:
            pass

    print(f"[*] Ingested {len(ecu_metadata)} distinct ECUs from simulation models.")
    print(f"[*] Ingested {len(dtc_dict)} diagnostic trouble code definitions.")

    # 2. Write data/dtc_database_mb.json
    with open(DTC_OUT, "w", encoding="utf-8") as f:
        json.dump(dtc_dict, f, indent=2, ensure_ascii=False)
    print(f"[+] Saved {DTC_OUT} ({os.path.getsize(DTC_OUT) // 1024} KB)")

    # 3. Enrich data/ecu_catalog.json
    catalog_data = {"metadata": {}, "ecus": {}}
    if os.path.exists(ECU_CATALOG_FILE):
        with open(ECU_CATALOG_FILE, "r", encoding="utf-8") as f:
            try:
                catalog_data = json.load(f)
            except Exception as e:
                print(f"[!] Warning reading existing catalog: {e}")

    catalog_map = catalog_data.get("ecus", {})

    updated_count = 0
    added_count = 0

    for name, meta in ecu_metadata.items():
        if name in catalog_map:
            # Update existing entry with CAN IDs or DTC counts if missing
            entry = catalog_map[name]
            if meta["tx_id"] and not entry.get("tx_id"):
                entry["tx_id"] = meta["tx_id"]
                updated_count += 1
            if meta["rx_id"] and not entry.get("rx_id"):
                entry["rx_id"] = meta["rx_id"]
            if meta["dtc_count"] > entry.get("dtc_count", 0):
                entry["dtc_count"] = meta["dtc_count"]
            for ch in meta["chassis"]:
                if ch not in entry.get("chassis", []):
                    entry.setdefault("chassis", []).append(ch)
        else:
            # Add new ECU entry
            catalog_map[name] = {
                "ecu_name": name,
                "protocol": meta["protocol"],
                "tx_id": meta["tx_id"] or "0x7e0",
                "rx_id": meta["rx_id"] or "0x7e8",
                "func_id": "0x7df",
                "dtc_count": meta["dtc_count"],
                "chassis": meta["chassis"],
            }
            added_count += 1

    # Sort ecus by key
    sorted_ecus = {k: catalog_map[k] for k in sorted(catalog_map.keys())}
    total = len(sorted_ecus)
    meta = catalog_data.get("metadata", {})
    meta["title"] = meta.get("title", "Sterngate Automotive ECU Diagnostic Index")
    meta["version"] = "2.1"
    meta["description"] = f"Lean canonical diagnostic routing index for {total} automotive electronic control units."
    meta["total_ecus"] = total
    meta["unique_ecus"] = total
    catalog_data["metadata"] = meta
    catalog_data["ecus"] = sorted_ecus

    # Write in the clean one-line-per-ecu format to keep it compact and fast to load
    with open(ECU_CATALOG_FILE, "w", encoding="utf-8") as f:
        f.write('{\n')
        f.write('  "metadata": {\n')
        f.write(f'    "title": "{meta["title"]}",\n')
        f.write(f'    "version": "{meta["version"]}",\n')
        f.write(f'    "description": "{meta["description"]}",\n')
        f.write(f'    "total_ecus": {total},\n')
        f.write(f'    "unique_ecus": {total}\n')
        f.write('  },\n')
        f.write('  "ecus": {\n')
        keys = list(sorted_ecus.keys())
        for idx, k in enumerate(keys):
            comma = "," if idx < len(keys) - 1 else ""
            f.write(f'    "{k}": {json.dumps(sorted_ecus[k], ensure_ascii=False)}{comma}\n')
        f.write('  }\n')
        f.write('}\n')

    print(f"[+] Updated {ECU_CATALOG_FILE}: {total} total ECUs (+{added_count} new, {updated_count} enriched).")

    # 4. Generate Chassis Profiles for Modern Platforms
    chassis_platforms = {
        "mercedes_w213_e_class": ("W213 E-Class (2016-2023)", "W213", ["MED177", "MED1775", "CR60NFZ", "CR61", "VGSNAG3", "ESP213_AMG", "ESP213_MOPF", "ACC_213", "EZS213"]),
        "mercedes_w205_c_class": ("W205 C-Class (2014-2021)", "W205", ["MED177", "CR43", "CR60NFZ", "VGSNAG2", "VGSNAG3", "ESP205", "ACC_205", "EZS205"]),
        "mercedes_w222_s_class": ("W222 S-Class (2013-2020)", "W222", ["MED177", "CR60NFZ", "VGSNAG3", "ABC222", "ABR222", "EPKB222", "EZS222"]),
        "mercedes_w223_s_class": ("W223 S-Class (2021-Present)", "W223", ["MED177", "CR61", "VGSNAG3", "ESP223", "AVAS223", "AD_FL223", "AD_FR223", "TPM223"]),
        "mercedes_w167_gle": ("W167 GLE / GLS (2019-Present)", "W167", ["CR60NFZ", "VGSNAG3", "ESP167", "EZS167", "PTC167"]),
        "mercedes_w177_a_class": ("W177 A-Class / CLA (2018-Present)", "W177", ["MED177", "CR60NFZ", "ESP177", "ESP177_AMG", "TPMMFA2", "EZS177"]),
        "mercedes_w290_amg_gt": ("W290 AMG GT 4-Door Coupe", "W290", ["MED177", "VGSNAG3", "ESP290_AMG", "AERO290", "AVAS290AMG"]),
        "mercedes_w463_g_class": ("W463 / W464 G-Class AMG", "W463", ["MED177", "VGSNAG3", "ESP464", "ESP465", "EZS463"]),
        "mercedes_w907_sprinter": ("W907 / W910 Sprinter (2018-Present)", "W907", ["CR60NFZ", "CRD3", "ACU907", "ACU907_IO1", "ESP907", "EZS907"]),
        "mercedes_w447_v_class": ("W447 V-Class / Vito (2014-Present)", "W447", ["CR60NFZ", "VGSNAG2", "VGSNAG3", "ESP447", "EZS447"]),
    }

    profiles_created = 0
    for prof_id, (full_name, chassis_code, target_ecus) in chassis_platforms.items():
        modules = {}
        parameters = []

        for ecu_key in target_ecus:
            # Find best matching metadata
            match_meta = ecu_metadata.get(ecu_key.upper())
            if not match_meta:
                # Try prefix search
                for k, v in ecu_metadata.items():
                    if k.startswith(ecu_key.upper()):
                        match_meta = v
                        break

            tx = "0x7E0"
            rx = "0x7E8"
            proto = "UDS"

            if match_meta:
                tx = match_meta["tx_id"] or tx
                rx = match_meta["rx_id"] or rx
                proto = match_meta["protocol"] or proto

                # Add DIDs from this ECU
                for d in match_meta["dids"][:8]: # top 8 diagnostic DIDs per module
                    param_id = f"{ecu_key.lower()}_{d['did'].replace('0x', '').lower()}"
                    d_trans = DID_TRANSLATIONS.get(d["name"], {})
                    parameters.append({
                        "id": param_id,
                        "name": d["name"],
                        "names": {
                            "en": d["name"],
                            "de": d_trans.get("de", d["name"]),
                            "sv": d_trans.get("sv", d["name"])
                        },
                        "module": ecu_key,
                        "service": 34, # 0x22 ReadDataByIdentifier
                        "did": d["did"],
                        "byte_offset": 0,
                        "length": 4,
                        "scaling": {"slope": 1.0, "offset": 0.0},
                        "unit": "",
                    })
            else:
                # Standard DIDs
                parameters.append({
                    "id": f"{ecu_key.lower()}_hw_id",
                    "name": f"{ecu_key} Hardware Number",
                    "names": {"en": f"{ecu_key} Hardware Number", "de": f"{ecu_key} Hardwarenummer", "sv": f"{ecu_key} Hårdvarunummer"},
                    "module": ecu_key,
                    "service": 34,
                    "did": "0xF191",
                    "byte_offset": 0,
                    "length": 10,
                    "scaling": {"slope": 1.0, "offset": 0.0},
                    "unit": "",
                })

            modules[ecu_key] = {
                "name": f"{ecu_key} ({full_name})",
                "names": {
                    "en": f"{ecu_key} Controller",
                    "de": f"{ecu_key} Steuergerät",
                    "sv": f"{ecu_key} Styrenhet"
                },
                "tx_id": tx,
                "rx_id": rx,
                "protocol": proto,
                "seed_key_algo": "DaimlerStandardLevel01" if "KWP" in proto else "DaimlerStandardLevel0B"
            }

        profile_json = {
            "profile_name": prof_id,
            "oem": "Mercedes-Benz",
            "chassis": chassis_code,
            "gateway_type": "DoIP / Central Gateway (CGW)",
            "default_bitrate": 500000,
            "modules": modules,
            "parameters": parameters,
        }

        out_path = os.path.join(PROFILES_OUT, f"{prof_id}.json")
        with open(out_path, "w", encoding="utf-8") as f:
            json.dump(profile_json, f, indent=2, ensure_ascii=False)
        profiles_created += 1

    print(f"[+] Created {profiles_created} factory chassis vehicle profiles in {PROFILES_OUT}!")
    print("[*] Ingestion pipeline complete!")

if __name__ == "__main__":
    main()
