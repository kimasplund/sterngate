use anyhow::Result;

use crate::args::EcuCommands;
use sterngate_core::EcuCatalog;

pub fn execute(action: EcuCommands) -> Result<()> {
    let catalog = match EcuCatalog::load_default() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ECU catalog error: {}.", e);
            return Ok(());
        }
    };

    match action {
        EcuCommands::Stats => {
            println!("============================================================");
            println!("  Mercedes-Benz ECU Diagnostic Catalog Statistics");
            println!("============================================================");
            let meta = catalog.stats();
            let title = meta
                .title
                .as_deref()
                .unwrap_or("Sterngate Native ECU Catalog");
            println!("  • Catalog Title:                     {}", title);
            println!(
                "  • Canonical ECU Definitions:         {}",
                if meta.total_ecus > 0 {
                    meta.total_ecus
                } else {
                    meta.unique_ecus
                }
            );
            if meta.total_cbf_files > 0 {
                println!(
                    "  • Legacy Source Files Indexed:       {}",
                    meta.total_cbf_files
                );
            }
        }
        EcuCommands::Search { query } => {
            println!("============================================================");
            println!("  Searching ECU Catalog for: '{}'", query);
            println!("============================================================");
            let results = catalog.search(&query, 50);
            for r in &results {
                let chassis_str = if r.chassis.is_empty() {
                    "Universal / Unspecified".to_string()
                } else {
                    r.chassis.join(", ")
                };
                println!(
                    "  • {:<16} | {:<7} | CAN Tx/Rx: {:<6} / {:<6} | Func: {:<5} | DTCs: {:<4} | Chassis: {}",
                    r.ecu_name,
                    r.protocol,
                    r.tx_id.as_deref().unwrap_or("N/A"),
                    r.rx_id.as_deref().unwrap_or("N/A"),
                    r.func_id.as_deref().unwrap_or("0x7DF"),
                    r.dtc_count,
                    chassis_str
                );
            }
            println!("\nFound {} matching ECU(s).", results.len());
        }
        EcuCommands::Inspect { ecu } => {
            if let Some(info) = catalog.get_ecu(&ecu) {
                println!("============================================================");
                println!("  ECU Diagnostic Definition: {}", info.ecu_name);
                println!("============================================================");
                println!("  • Protocol:          {}", info.protocol);
                println!(
                    "  • Physical Tx CAN:   {}",
                    info.tx_id.as_deref().unwrap_or("N/A")
                );
                println!(
                    "  • Physical Rx CAN:   {}",
                    info.rx_id.as_deref().unwrap_or("N/A")
                );
                println!(
                    "  • Functional ID:     {}",
                    info.func_id.as_deref().unwrap_or("0x7DF")
                );
                println!("  • Known DTC Codes:   {}", info.dtc_count);
                println!(
                    "\n  Supported Chassis Platforms ({} total):",
                    info.chassis.len()
                );
                for c in &info.chassis {
                    println!("    - {}", c);
                }
            } else {
                eprintln!("ECU '{}' not found in catalog.", ecu);
            }
        }
    }
    Ok(())
}
