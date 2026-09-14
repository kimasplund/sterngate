#!/usr/bin/env python3
"""
scripts/ingest_xentry.py
Extracts and normalizes all diagnostic databases (.cbf, .smr-d, simulation XML)
from XENTRY Diagnostics OpenShell installation archives into Sterngate.
"""

import os
import sys
import zipfile
import subprocess
import shutil

ZIP_PATH = "reference/xentry/XENTRY Diagnostics OpenShell_26.9.4.zip"
OUT_BASE = "reference/xentry/extracted_diagnostics"
CBF_OUT = os.path.join(OUT_BASE, "cbf")
SMRD_OUT = os.path.join(OUT_BASE, "smrd")
SIM_OUT = os.path.join(OUT_BASE, "simulations")

TARGET_MSIS = [
    # CBF & SMR-D payload MSIs identified in archive scan
    "data/Media1/{d3ae1deb-5faa-4fea-831c-26133ba1d7a7}/d3ae1deb-5faa-4fea-831c-26133ba1d7a7.msi",
    "data/Media1/{f65b77da-93f3-4893-9207-c4bf2faa5491}/f65b77da-93f3-4893-9207-c4bf2faa5491.msi",
    "data/Media3/{700280e0-1dbd-4042-b4ca-decc6cdf10ec}/700280e0-1dbd-4042-b4ca-decc6cdf10ec.msi",
    "data/Media3/{b238aef3-7fb4-4ee8-bb39-e33751bf16d2}/b238aef3-7fb4-4ee8-bb39-e33751bf16d2.msi",
    "data/Media1/{02f69e77-fb5b-490f-9a61-b80973dc9d89}/02f69e77-fb5b-490f-9a61-b80973dc9d89.msi",
    "data/Media2/{5ff0fa99-2ab7-44e9-b2db-77fc0073bc33}/5ff0fa99-2ab7-44e9-b2db-77fc0073bc33.msi",
    "data/Media1/{7f6cd4c3-add6-460a-bde9-43e9fccbd3ad}/7f6cd4c3-add6-460a-bde9-43e9fccbd3ad.msi",
    "data/Media2/{909ccec0-dce2-4100-9483-4dc48afd932c}/909ccec0-dce2-4100-9483-4dc48afd932c.msi",
    "data/Media1/{4c787450-aace-4322-a72a-cf9d27d78727}/4c787450-aace-4322-a72a-cf9d27d78727.msi",
]

def run_7z_extract_patterns(cab_path, raw_out):
    patterns = ["*.cbf*", "*.smr*", "*simulation.xml*", "*.sim*"]
    for pat in patterns:
        subprocess.run(
            ["7z", "e", cab_path, f"-o{raw_out}", pat, "-y"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

def main():
    if not os.path.exists(ZIP_PATH):
        print(f"Error: Archive not found: {ZIP_PATH}", file=sys.stderr)
        sys.exit(1)

    os.makedirs(CBF_OUT, exist_ok=True)
    os.makedirs(SMRD_OUT, exist_ok=True)
    os.makedirs(SIM_OUT, exist_ok=True)

    print(f"[*] Opening XENTRY archive: {ZIP_PATH}...")
    zf = zipfile.ZipFile(ZIP_PATH)

    total_cbf = 0
    total_smrd = 0
    total_sim = 0

    temp_root = "/tmp/xentry_extract"
    os.makedirs(temp_root, exist_ok=True)

    for i, msi_rel in enumerate(TARGET_MSIS, 1):
        guid = msi_rel.split("/")[2].strip("{}")
        print(f"\n[{i}/{len(TARGET_MSIS)}] Processing MSI: {guid}...")
        msi_temp = os.path.join(temp_root, f"{guid}.msi")

        try:
            with zf.open(msi_rel) as src, open(msi_temp, "wb") as dst:
                shutil.copyfileobj(src, dst)

            cab_temp = os.path.join(temp_root, f"{guid}_cab")
            os.makedirs(cab_temp, exist_ok=True)
            subprocess.run(
                ["7z", "e", msi_temp, f"-o{cab_temp}", "Data1.cab", "-y"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )

            cab_file = os.path.join(cab_temp, "Data1.cab")
            if not os.path.exists(cab_file):
                print(f"  [!] No Data1.cab found in {guid}")
                continue

            raw_out = os.path.join(temp_root, f"{guid}_raw")
            os.makedirs(raw_out, exist_ok=True)
            run_7z_extract_patterns(cab_file, raw_out)

            msi_cbf = 0
            msi_smrd = 0
            msi_sim = 0

            for fname in os.listdir(raw_out):
                fpath = os.path.join(raw_out, fname)
                lower = fname.lower()

                if ".cbf" in lower:
                    clean_name = fname.split(".cbf")[0] + ".cbf"
                    dst = os.path.join(CBF_OUT, clean_name)
                    shutil.copy2(fpath, dst)
                    msi_cbf += 1
                elif ".smr_d" in lower or ".smr-d" in lower or ".smrd" in lower:
                    clean_name = fname.split(".smr")[0] + ".smr-d"
                    dst = os.path.join(SMRD_OUT, clean_name)
                    shutil.copy2(fpath, dst)
                    msi_smrd += 1
                elif "simulation.xml" in lower or ".sim" in lower:
                    clean_name = fname
                    parts = fname.split(".")
                    if len(parts) >= 3 and len(parts[-1]) >= 20:
                        clean_name = ".".join(parts[:-1])
                    dst = os.path.join(SIM_OUT, clean_name)
                    shutil.copy2(fpath, dst)
                    msi_sim += 1

            total_cbf += msi_cbf
            total_smrd += msi_smrd
            total_sim += msi_sim
            print(f"  -> Extracted: {msi_cbf} CBFs, {msi_smrd} SMR-Ds, {msi_sim} Simulations")

        except Exception as e:
            print(f"  [!] Error processing {guid}: {e}")
        finally:
            shutil.rmtree(os.path.join(temp_root, f"{guid}_cab"), ignore_errors=True)
            shutil.rmtree(os.path.join(temp_root, f"{guid}_raw"), ignore_errors=True)
            if os.path.exists(msi_temp):
                os.remove(msi_temp)

    shutil.rmtree(temp_root, ignore_errors=True)

    print("\n" + "=" * 60)
    print("XENTRY DIAGNOSTICS EXTRACTION SUMMARY:")
    print(f"  • Total CBF Databases:        {total_cbf}")
    print(f"  • Total SMR-D Containers:     {total_smrd}")
    print(f"  • Total Simulation Models:    {total_sim}")
    print(f"  • Output Directory:           {OUT_BASE}")
    print("=" * 60)

if __name__ == "__main__":
    main()

