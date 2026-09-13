#!/usr/bin/env python3
"""
Sterngate CBF Deduplication, Parsing & Cataloging Engine
Analyzes all 2,055 Daimler Vediamo CBF files in data/cbf/:
- Identifies exact byte-for-byte duplicates (identical SHA256)
- Tracks chronological versions (same ECU updated across years)
- Extracts ECU names, translation dates, protocols (UDS/KWP2000), CAN arbitration IDs, and presentations
- Selects canonical latest versions
- Generates data/cbf_catalog.json
"""

import os
import re
import json
import time
import struct
import hashlib
from datetime import datetime
from collections import defaultdict
from typing import Dict, List, Any, Optional

def parse_date(date_str: str) -> Optional[str]:
    """Converts DD.MM.YYYY to YYYY-MM-DD for sorting."""
    if not date_str:
        return None
    try:
        parts = date_str.split(".")
        if len(parts) == 3:
            day, month, year = int(parts[0]), int(parts[1]), int(parts[2])
            return f"{year:04d}-{month:02d}-{day:02d}"
    except Exception:
        pass
    return None

def extract_can_ids(data: bytes) -> Dict[str, Any]:
    """Extracts CAN arbitration IDs from CBF parameter tables."""
    found_ids = []
    # Standard 11-bit diagnostic IDs: 0x600 - 0x7FF
    for can_id in range(0x600, 0x7FF):
        b = struct.pack("<I", can_id)
        idx = 0
        while True:
            idx = data.find(b, idx)
            if idx == -1:
                break
            # In CBF tables, ID value is preceded by length 4: [0x04, 0x00, 0x00, 0x00]
            if idx >= 8 and data[idx-8:idx-4] == b"\x04\x00\x00\x00":
                found_ids.append(can_id)
            idx += 4

    unique_ids = sorted(list(set(found_ids)))
    
    tx_id = None
    rx_id = None
    func_id = None
    
    for cid in unique_ids:
        if cid == 0x7DF or cid == 0x7D0:
            func_id = hex(cid)
        elif 0x7E0 <= cid <= 0x7E7 or cid in [0x784, 0x662]:
            if not tx_id:
                tx_id = hex(cid)
        elif 0x7E8 <= cid <= 0x7EF or cid in [0x785, 0x663]:
            if not rx_id:
                rx_id = hex(cid)

    # Fallback to standard pairs if detected
    if not tx_id and len(unique_ids) >= 1:
        tx_id = hex(unique_ids[0])
    if not rx_id and len(unique_ids) >= 2:
        rx_id = hex(unique_ids[1])

    return {
        "tx_id": tx_id,
        "rx_id": rx_id,
        "func_id": func_id,
        "detected_can_ids": [hex(c) for c in unique_ids]
    }

def analyze_cbf_file(path: str) -> Dict[str, Any]:
    """Performs deep extraction of a single CBF file."""
    with open(path, "rb") as fp:
        data = fp.read()
    
    sha = hashlib.sha256(data).hexdigest()
    size = len(data)
    
    header_str = data[:512].decode("latin-1", errors="ignore")
    
    # 1. Translation Date
    date_m = re.search(r"DATE:([0-9\.]+)", header_str)
    raw_date = date_m.group(1) if date_m else ""
    iso_date = parse_date(raw_date) or "Unknown"
    
    # 2. Translator and GPD version
    trans_ver = re.search(r"CBF-TRANSLATOR-VERSION:([^\.]+)", header_str)
    gpd_ver = re.search(r"GPD-TRANSLATOR-VERSION:([^\.]+)", header_str)
    target_rel = re.search(r"TARGET-RELEASE:([^\.]+)", header_str)
    
    # 3. Protocol
    proto = "KWP2000"
    if b"ISO 14229" in data[:50000] or b"UDS" in data[:50000] or b"KW2C3PE" in data[:50000]:
        proto = "UDS"
    elif b"KLINE" in data[:50000] and b"KW2" not in data[:50000]:
        proto = "K-Line"
        
    # 4. CAN Routing
    routing = extract_can_ids(data)
    
    # 5. Diagnostic Presentations (scaling formulas)
    presentations = set(re.findall(rb"PRES_([A-Za-z0-9_]+)", data))
    
    # 6. DTC Codes
    dtcs = set(re.findall(rb"([PBCU][0-9A-Fa-f]{4})", data))
    
    # 7. Chassis Name from directory hierarchy
    rel_path = os.path.relpath(path, "data/cbf")
    parts = rel_path.split(os.sep)
    chassis_dir = parts[0] if len(parts) > 1 else "Unknown"

    return {
        "path": path,
        "rel_path": rel_path,
        "chassis": chassis_dir,
        "size": size,
        "sha256": sha,
        "raw_date": raw_date,
        "iso_date": iso_date,
        "translator_version": trans_ver.group(1) if trans_ver else "Unknown",
        "gpd_version": gpd_ver.group(1) if gpd_ver else "Unknown",
        "target_release": target_rel.group(1) if target_rel else "Unknown",
        "protocol": proto,
        "can_routing": routing,
        "presentation_count": len(presentations),
        "dtc_count": len(dtcs)
    }

def main():
    start_time = time.time()
    cbf_dir = "data/cbf"
    
    if not os.path.exists(cbf_dir):
        print(f"Error: Directory {cbf_dir} does not exist.")
        return

    print("==================================================================")
    print("  Sterngate CBF Deduplication & Diagnostic Cataloging Engine")
    print("==================================================================")
    print(f"Scanning and analyzing CBF files in '{cbf_dir}'...")

    file_list = []
    for root, dirs, files in os.walk(cbf_dir):
        for f in files:
            if f.lower().endswith(".cbf"):
                file_list.append(os.path.join(root, f))

    total_files = len(file_list)
    print(f"Discovered {total_files} total CBF files across all chassis folders.")

    # Group by ECU Base Name
    ecu_catalog = defaultdict(lambda: {
        "ecu_name": "",
        "unique_versions_count": 0,
        "total_files_count": 0,
        "versions": {},  # sha256 -> version_info
        "chassis_map": defaultdict(list)
    })

    by_sha256 = defaultdict(list)

    analyzed_count = 0
    for path in file_list:
        fname = os.path.basename(path)
        ecu_name = os.path.splitext(fname)[0].upper()
        
        info = analyze_cbf_file(path)
        sha = info["sha256"]
        by_sha256[sha].append(info)
        
        rec = ecu_catalog[ecu_name]
        rec["ecu_name"] = ecu_name
        rec["total_files_count"] += 1
        rec["chassis_map"][info["chassis"]].append(sha)
        
        if sha not in rec["versions"]:
            rec["versions"][sha] = {
                "sha256": sha,
                "size": info["size"],
                "date": info["raw_date"],
                "iso_date": info["iso_date"],
                "protocol": info["protocol"],
                "gpd_version": info["gpd_version"],
                "tx_id": info["can_routing"]["tx_id"],
                "rx_id": info["can_routing"]["rx_id"],
                "func_id": info["can_routing"]["func_id"],
                "presentation_count": info["presentation_count"],
                "dtc_count": info["dtc_count"],
                "occurrences": [info["rel_path"]],
                "chassis_list": [info["chassis"]]
            }
        else:
            rec["versions"][sha]["occurrences"].append(info["rel_path"])
            if info["chassis"] not in rec["versions"][sha]["chassis_list"]:
                rec["versions"][sha]["chassis_list"].append(info["chassis"])

        analyzed_count += 1
        if analyzed_count % 500 == 0 or analyzed_count == total_files:
            print(f"  Processed {analyzed_count}/{total_files} files ({(analyzed_count/total_files)*100:.1f}%)...")

    # Determine Canonical (Latest) Version for Each ECU
    clean_catalog = {}
    for ecu_name, rec in ecu_catalog.items():
        rec["unique_versions_count"] = len(rec["versions"])
        
        # Sort versions chronologically: newest date first, then largest size
        sorted_versions = sorted(
            rec["versions"].values(),
            key=lambda v: (v["iso_date"], v["size"]),
            reverse=True
        )
        canonical = sorted_versions[0]
        
        clean_catalog[ecu_name] = {
            "ecu_name": ecu_name,
            "canonical_version": {
                "sha256": canonical["sha256"],
                "date": canonical["date"],
                "iso_date": canonical["iso_date"],
                "size_bytes": canonical["size"],
                "protocol": canonical["protocol"],
                "tx_id": canonical["tx_id"],
                "rx_id": canonical["rx_id"],
                "func_id": canonical["func_id"],
                "presentation_count": canonical["presentation_count"],
                "dtc_count": canonical["dtc_count"],
                "primary_path": canonical["occurrences"][0]
            },
            "total_copies_in_cbf": rec["total_files_count"],
            "distinct_versions_count": rec["unique_versions_count"],
            "all_chassis_supported": sorted(list(rec["chassis_map"].keys())),
            "version_history": sorted_versions
        }

    elapsed = time.time() - start_time

    # Output statistics
    unique_ecu_count = len(clean_catalog)
    unique_sha_count = len(by_sha256)
    duplicate_groups = {s: items for s, items in by_sha256.items() if len(items) > 1}
    redundant_copies = sum(len(items) - 1 for items in duplicate_groups.values())
    
    single_version_ecus = [e for e in clean_catalog.values() if e["distinct_versions_count"] == 1]
    multi_version_ecus = [e for e in clean_catalog.values() if e["distinct_versions_count"] > 1]

    # Save to JSON
    output_catalog_path = "data/cbf_catalog.json"
    os.makedirs("data", exist_ok=True)
    with open(output_catalog_path, "w", encoding="utf-8") as fp:
        json.dump({
            "metadata": {
                "generated_at": datetime.utcnow().isoformat() + "Z",
                "total_cbf_files": total_files,
                "unique_ecus": unique_ecu_count,
                "unique_sha256_hashes": unique_sha_count,
                "exact_duplicate_groups": len(duplicate_groups),
                "redundant_file_copies": redundant_copies,
                "processing_time_seconds": round(elapsed, 2)
            },
            "ecus": clean_catalog
        }, fp, indent=2)

    print("\n==================================================================")
    print("  DEDUPLICATION & PARSING RESULTS SUMMARY")
    print("==================================================================")
    print(f"• Total CBF Files Scanned:           {total_files}")
    print(f"• Unique ECU Types:                  {unique_ecu_count}")
    print(f"• Unique Binary Content Hashes:      {unique_sha_count}")
    print(f"• Content-Identical Duplicate Groups: {len(duplicate_groups)}")
    print(f"• Redundant File Copies:             {redundant_copies} ({(redundant_copies/total_files)*100:.1f}% of entire database)")
    print(f"• Static Single-Version ECUs:        {len(single_version_ecus)} ({(len(single_version_ecus)/unique_ecu_count)*100:.1f}%)")
    print(f"• Updated Multi-Version ECUs:        {len(multi_version_ecus)} ({(len(multi_version_ecus)/unique_ecu_count)*100:.1f}%)")
    print(f"• Execution Time:                    {elapsed:.2f} seconds")
    print(f"• Saved Canonical Catalog To:        {output_catalog_path}")

    print("\nTop 10 Most Widely Shared ECUs Across Chassis Directories:")
    sorted_by_chassis = sorted(clean_catalog.values(), key=lambda e: len(e["all_chassis_supported"]), reverse=True)
    for ecu in sorted_by_chassis[:10]:
        print(f"  • {ecu['ecu_name']:<18} | {len(ecu['all_chassis_supported']):>2} chassis folders | {ecu['distinct_versions_count']} version(s) | Proto: {ecu['canonical_version']['protocol']:<7} | CAN: {ecu['canonical_version']['tx_id'] or 'N/A'}/{ecu['canonical_version']['rx_id'] or 'N/A'}")

    print("\nTop 10 Most Evolved / Updated ECUs Over Time:")
    sorted_by_versions = sorted(clean_catalog.values(), key=lambda e: e["distinct_versions_count"], reverse=True)
    for ecu in sorted_by_versions[:10]:
        latest = ecu["canonical_version"]
        oldest = ecu["version_history"][-1]
        print(f"  • {ecu['ecu_name']:<18} | {ecu['distinct_versions_count']} distinct versions | Oldest: {oldest['date']} -> Latest: {latest['date']} | Size: {latest['size_bytes'] / 1024 / 1024:.1f} MB")

    print("==================================================================")

if __name__ == "__main__":
    main()

