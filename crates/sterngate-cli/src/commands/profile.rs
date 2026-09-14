use anyhow::Result;
use tracing::info;

use crate::args::{Cli, ProfileCommands};
use crate::commands::common::open_interface;
use sterngate_core::{EcuCatalog, VehicleProfile};
use sterngate_protocol::{BusDiscoverer, ProfileImporter};

pub async fn execute(action: ProfileCommands, cli: &Cli) -> Result<()> {
    match action {
        ProfileCommands::List => {
            println!("============================================================");
            println!("  Sterngate Installed Vehicle Profiles");
            println!("============================================================");
            let files = VehicleProfile::discover_paths("profiles");
            let mut found = 0;
            for path in files {
                if let Ok(prof) = VehicleProfile::load_from_file(&path) {
                    found += 1;
                    println!(
                        "  • {:<32} | {:<14} | {} ({} modules, {} DIDs)",
                        prof.profile_name,
                        prof.oem,
                        prof.chassis,
                        prof.modules.len(),
                        prof.parameters.len()
                    );
                    println!("    Path: {}", path.display());
                }
            }
            if found == 0 {
                println!("  No vehicle profiles found in profiles/");
            }
            Ok(())
        }
        ProfileCommands::Inspect { path } => {
            let prof = VehicleProfile::load_from_file(&path)?;
            println!("============================================================");
            println!("  Profile: {} ({})", prof.profile_name, prof.oem);
            println!(
                "  Chassis: {} | Gateway: {:?}",
                prof.chassis, prof.gateway_type
            );
            println!("  Default Bitrate: {} bps", prof.default_bitrate);
            println!("============================================================");
            println!("\nECU Modules:");
            for (mod_id, m) in &prof.modules {
                println!(
                    "  [{:<8}] {:<42} | Tx: {:<6} Rx: {:<6} | Protocol: {}",
                    mod_id, m.name, m.tx_id, m.rx_id, m.protocol
                );
            }
            println!(
                "\nDiagnostic Parameters ({} defined):",
                prof.parameters.len()
            );
            for p in &prof.parameters {
                println!(
                    "  • {:<16} DID: {:<6} ({}): [{:<20}] scale: *{} +{} {}",
                    p.id, p.did, p.module, p.name, p.scaling.slope, p.scaling.offset, p.unit
                );
            }
            Ok(())
        }
        ProfileCommands::Generate {
            out,
            oem,
            chassis,
            name,
        } => {
            info!(
                "Interrogating CAN bus to generate profile for {} {}...",
                oem, chassis
            );
            let mut iface = open_interface(&cli.can_interface).await;
            let catalog = EcuCatalog::load_default().ok();
            let discovered =
                BusDiscoverer::discover_ecus(iface.as_mut(), 0x700..=0x7EF, 25, catalog.as_ref())
                    .await?;

            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let profile = BusDiscoverer::generate_profile(&discovered, &oem, &chassis, &name);
            let json = serde_json::to_string_pretty(&profile)?;
            std::fs::write(&out, json)?;
            println!(
                "✓ Successfully generated vehicle profile with {} modules: {}",
                profile.modules.len(),
                out.display()
            );
            Ok(())
        }
        ProfileCommands::Import { input, output } => {
            println!("============================================================");
            println!("  Sterngate SMR-D & CBF Profile Importer");
            println!("============================================================");
            println!("  Input:  {}", input.display());
            println!("  Output: {}", output.display());
            println!("------------------------------------------------------------");

            let result = ProfileImporter::import_from_path(&input, &output)?;
            println!("  [SUCCESS] Ingestion completed:");
            println!(
                "    • Total files scanned:   {}",
                result.total_files_scanned
            );
            println!("    • CBF archives found:    {}", result.cbf_files_found);
            println!("    • SMR-D archives found:  {}", result.smrd_files_found);
            println!(
                "    • Profiles generated:    {}",
                result.profiles_generated.len()
            );
            for name in &result.profiles_generated {
                println!("      - {}", name);
            }
            if !result.warnings.is_empty() {
                println!("\n    Warnings / Skipped ({}):", result.warnings.len());
                for w in result.warnings.iter().take(10) {
                    println!("      ! {}", w);
                }
                if result.warnings.len() > 10 {
                    println!("      ... and {} more warnings", result.warnings.len() - 10);
                }
            }
            println!("============================================================");
            Ok(())
        }
    }
}
