use anyhow::Result;

use crate::args::VehicleCommands;
use sterngate_core::VehicleGarage;

pub fn execute(action: VehicleCommands) -> Result<()> {
    let garage = VehicleGarage::new(VehicleGarage::default_path());
    match action {
        VehicleCommands::List => {
            println!("============================================================");
            println!("  Sterngate Vehicle Garage");
            println!("============================================================");
            let vehicles = garage.list_vehicles()?;
            if vehicles.is_empty() {
                println!("  No vehicles found in garage. Run 'sterngate diag scan' to interrogate connected vehicle.");
            } else {
                for v in &vehicles {
                    println!(
                        "  • {:<18} | {:<22} | {} | Scans: {}",
                        v.vin, v.decoded.model_name, v.decoded.body_style, v.scan_count
                    );
                    println!(
                        "    Engine: {} | Last Scanned: {}",
                        v.decoded.engine, v.last_scanned
                    );
                }
            }
            Ok(())
        }
        VehicleCommands::Inspect { vin } => {
            if let Some(v) = garage.load_vehicle(&vin)? {
                println!("============================================================");
                println!("  Vehicle Profile: {}", v.vin);
                println!("============================================================");
                println!(
                    "  • Model:            {} ({})",
                    v.decoded.model_name, v.decoded.body_style
                );
                println!("  • Engine:           {}", v.decoded.engine);
                println!("  • Manufacturer:     {}", v.decoded.manufacturer);
                println!("  • First Scanned:    {}", v.first_scanned);
                println!("  • Last Scanned:     {}", v.last_scanned);
                println!("  • Total Scans:      {}", v.scan_count);
                if let Some(odo) = v.odometer_km {
                    println!("  • Odometer:         {} km", odo);
                }
                if let Some(volt) = v.battery_voltage {
                    println!("  • Battery:          {:.1} V", volt);
                }
                println!("\n  Detected ECU Modules ({}):", v.detected_modules.len());
                for (m_name, m_info) in &v.detected_modules {
                    println!(
                        "    - {:<10} | Part: {:<16} | HW: {:<12} | CAN: {:?}/{:?}",
                        m_name,
                        m_info.part_number.as_deref().unwrap_or("N/A"),
                        m_info.hardware_version.as_deref().unwrap_or("N/A"),
                        m_info.can_tx_id.as_deref().unwrap_or("N/A"),
                        m_info.can_rx_id.as_deref().unwrap_or("N/A")
                    );
                }
            } else {
                eprintln!("Vehicle '{}' not found in garage.", vin);
            }
            Ok(())
        }
        VehicleCommands::History { vin } => {
            println!("============================================================");
            println!("  Configuration Git Commit History: {}", vin);
            println!("============================================================");
            let history = garage.get_history(&vin)?;
            if history.is_empty() {
                println!("  No git history found for vehicle '{}'.", vin);
            } else {
                for c in &history {
                    println!("  commit {}", c.hash);
                    println!("  Date:   {}", c.date);
                    println!("  Author: {}", c.author);
                    println!("    {}\n", c.message);
                }
            }
            Ok(())
        }
        VehicleCommands::Rollback { vin, commit } => {
            println!("Rolling back vehicle {} to commit {}...", vin, commit);
            garage.rollback(&vin, &commit)?;
            println!(
                "✓ Rollback complete! Configuration reverted to commit {}.",
                commit
            );
            Ok(())
        }
    }
}
