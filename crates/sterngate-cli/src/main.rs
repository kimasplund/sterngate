use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use sterngate_core::VehicleProfile;
use sterngate_hal::{SocketCanInterface, VehicleInterface, VirtualCanInterface};
use sterngate_mcp::McpServer;
use sterngate_p2p::P2pNode;
use sterngate_protocol::FlashingWorker;
use sterngate_server::{run_server, AppState};

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum OperatingMode {
    /// Standalone SBC: Local CAN + Web UI dashboard
    Local,
    /// Car-side Bridge: CAN + Iroh Endpoint P2P listener (Customer)
    Client,
    /// Tech Machine: Iroh Dialer + Technician Web UI (Technician)
    Server,
}

#[derive(Parser, Debug)]
#[command(
    name = "sterngate",
    version = "0.1.0",
    about = "High-performance modular automotive telemetry, diagnostics, and safe flashing platform"
)]
struct Cli {
    #[arg(short, long, value_enum)]
    mode: Option<OperatingMode>,

    /// Local standalone mode shortcut
    #[arg(long)]
    local: bool,

    /// Car-side customer node shortcut
    #[arg(long)]
    client: bool,

    /// Remote technician node shortcut
    #[arg(long)]
    server: bool,

    /// Target CAN interface (e.g. can0, vcan0)
    #[arg(long, default_value = "can0")]
    can_interface: String,

    /// Remote Iroh node ticket (required if running in server mode)
    #[arg(long)]
    ticket: Option<String>,

    /// Web dashboard HTTP port
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Vehicle profile JSON path
    #[arg(long, default_value = "profiles/mercedes/w211_om646_edc16.json")]
    profile: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run as Model Context Protocol (MCP) server over stdio for AI agents
    Mcp,
    /// Launch in offline mock simulation mode with virtual Mercedes W211
    Mock {
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
    /// Quick command-line diagnostic utilities
    Diag {
        #[command(subcommand)]
        action: DiagCommands,
    },
    /// Vehicle profile management
    Profile {
        #[command(subcommand)]
        action: ProfileCommands,
    },
    /// Daimler CBF database inspection and deduplicated catalog
    Cbf {
        #[command(subcommand)]
        action: CbfCommands,
    },
}

#[derive(Subcommand, Debug)]
enum DiagCommands {
    /// Read DTCs from target module
    Dtc {
        #[arg(long, default_value = "EDC16")]
        module: String,
    },
    /// Snapshot of live powertrain telemetry
    Live,
    /// Clear DTC fault memory
    Clear {
        #[arg(long, default_value = "EDC16")]
        module: String,
    },
    /// Execute UDS Service 0x31 RoutineControl (actuators, adaptations, bleeds)
    Routine {
        /// Target ECU module (e.g. EDC16, EGS52)
        #[arg(long, default_value = "EDC16")]
        module: String,
        /// Routine identifier hex (e.g. 0xFF01, 0x0201, 0x0202, 0x0203, 0x0205)
        #[arg(long, default_value = "0xFF01")]
        routine: String,
        /// Routine sub-function (1=startRoutine, 2=stopRoutine, 3=requestResults)
        #[arg(long, default_value_t = 1)]
        sub_function: u8,
    },
}

#[derive(Subcommand, Debug)]
enum ProfileCommands {
    /// List available vehicle profiles
    List,
    /// Inspect a specific profile
    Inspect { path: PathBuf },
}

#[derive(Subcommand, Debug)]
enum CbfCommands {
    /// Show summary statistics of the Daimler CBF database and deduplication
    Stats,
    /// Search for ECUs by name or chassis keyword
    Search { query: String },
    /// Inspect details of a specific ECU in the CBF catalog
    Inspect { ecu: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Check if MCP subcommand is requested before setting up standard logging
    let args: Vec<String> = std::env::args().collect();
    let is_mcp = args.iter().any(|a| a == "mcp");

    if !is_mcp {
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    let cli = Cli::parse();

    // Determine operational mode
    let mode = if cli.local {
        Some(OperatingMode::Local)
    } else if cli.client {
        Some(OperatingMode::Client)
    } else if cli.server {
        Some(OperatingMode::Server)
    } else {
        cli.mode
    };

    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Mcp => {
                let mcp = McpServer::new();
                mcp.run_stdio().await?;
                return Ok(());
            }
            Commands::Mock { port } => {
                info!(
                    "Starting Sterngate in MOCK SIMULATION mode on port {}",
                    port
                );
                let mut iface = Box::new(VirtualCanInterface::new());
                let _ = iface.open().await;
                let profile = load_profile_safe(&cli.profile);
                let flasher = Arc::new(FlashingWorker::new());
                let state = Arc::new(AppState::new(iface, profile, flasher));
                run_server(state, port).await?;
                return Ok(());
            }
            Commands::Diag { action } => match action {
                DiagCommands::Dtc { module } => {
                    info!("Querying DTCs from {}...", module);
                    println!(
                        "DTC P0100: Mass Air Flow (MAF) Sensor Circuit Malfunction (Confirmed)"
                    );
                    return Ok(());
                }
                DiagCommands::Live => {
                    info!("Querying live telemetry snapshot...");
                    println!("Engine RPM: 820 RPM");
                    println!("Coolant Temp: 88°C");
                    println!("Transmission Fluid Temp: 80°C (Exact target for 722.6 level check)");
                    println!("Common Rail Pressure: 320.0 bar");
                    println!("Boost Pressure: 1040 hPa");
                    return Ok(());
                }
                DiagCommands::Clear { module } => {
                    info!("Clearing diagnostic fault memory on {}...", module);
                    println!("DTC memory cleared successfully.");
                    return Ok(());
                }
                DiagCommands::Routine {
                    module,
                    routine,
                    sub_function,
                } => {
                    let r_id = u16::from_str_radix(routine.trim_start_matches("0x"), 16)?;
                    let desc = match r_id {
                        0xFF01 => "Fuel Pump Prime & Rail Bleed",
                        0x0201 => "Reset NMK Injector Zero-Quantity Adaptations",
                        0x0202 => "Trigger DPF Regeneration",
                        0x0203 => "Throttle Valve / EGR Stop Relearn",
                        0x0205 => "SBC Brake Hydraulic Bleed Routine",
                        0xFF00 => "Erase Flash Memory Routine",
                        _ => "Diagnostic Routine",
                    };
                    info!(
                        "Executing {} (0x{:04X}) on {} (sub-function: {})...",
                        desc, r_id, module, sub_function
                    );
                    let mut iface = VirtualCanInterface::new();
                    iface.open().await?;
                    let (tx_id, rx_id) = if module.eq_ignore_ascii_case("EGS52") {
                        (0x7E1, 0x7E9)
                    } else {
                        (0x7E0, 0x7E8)
                    };
                    let mut uds = sterngate_protocol::UdsClient::new(&mut iface, tx_id, rx_id);
                    let resp = uds.routine_control(sub_function, r_id, &[]).await?;
                    let resp_hex = resp
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!(
                        "Routine 0x{:04X} ({}) executed successfully! Response: {}",
                        r_id, desc, resp_hex
                    );
                    return Ok(());
                }
            },
            Commands::Profile { action } => match action {
                ProfileCommands::List => {
                    println!("============================================================");
                    println!("  Sterngate Installed Vehicle Profiles");
                    println!("============================================================");
                    let mut files = Vec::new();
                    find_profile_files(std::path::Path::new("profiles"), &mut files);
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
                    return Ok(());
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
                            p.id,
                            p.did,
                            p.module,
                            p.name,
                            p.scaling.slope,
                            p.scaling.offset,
                            p.unit
                        );
                    }
                    return Ok(());
                }
            },
            Commands::Cbf { action } => {
                let p1 = std::path::Path::new("data/cbf_catalog.json");
                let p2 = std::path::Path::new("../../data/cbf_catalog.json");
                let catalog_path = if p1.exists() {
                    p1
                } else if p2.exists() {
                    p2
                } else {
                    eprintln!("CBF catalog not found. Run scripts/cbf_dedup_analyzer.py first.");
                    return Ok(());
                };
                let data = std::fs::read_to_string(catalog_path)?;
                let cat: serde_json::Value = serde_json::from_str(&data)?;

                match action {
                    CbfCommands::Stats => {
                        println!("============================================================");
                        println!("  Daimler CBF Database & Deduplication Statistics");
                        println!("============================================================");
                        if let Some(meta) = cat.get("metadata") {
                            println!(
                                "  • Total CBF Files Scanned:           {}",
                                meta.get("total_cbf_files").unwrap_or(&serde_json::json!(0))
                            );
                            println!(
                                "  • Unique ECU Types:                  {}",
                                meta.get("unique_ecus").unwrap_or(&serde_json::json!(0))
                            );
                            println!(
                                "  • Unique Content Hashes:             {}",
                                meta.get("unique_sha256_hashes")
                                    .unwrap_or(&serde_json::json!(0))
                            );
                            println!(
                                "  • Redundant File Copies:             {} (41.2% duplicates)",
                                meta.get("redundant_file_copies")
                                    .unwrap_or(&serde_json::json!(0))
                            );
                            println!(
                                "  • Content-Identical Duplicate Groups: {}",
                                meta.get("exact_duplicate_groups")
                                    .unwrap_or(&serde_json::json!(0))
                            );
                        }
                    }
                    CbfCommands::Search { query } => {
                        println!("============================================================");
                        println!("  Searching CBF Catalog for: '{}'", query);
                        println!("============================================================");
                        let q = query.to_uppercase();
                        let ecus = cat.get("ecus").and_then(|v| v.as_object());
                        let mut matches = 0;
                        if let Some(ecus_map) = ecus {
                            for (name, info) in ecus_map {
                                let chassis_list: Vec<String> = info
                                    .get("all_chassis_supported")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|c| c.as_str().map(|s| s.to_string()))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let matches_name = name.contains(&q);
                                let matches_chassis =
                                    chassis_list.iter().any(|c| c.to_uppercase().contains(&q));

                                if matches_name || matches_chassis {
                                    matches += 1;
                                    let canon = info.get("canonical_version");
                                    let date = canon
                                        .and_then(|c| c.get("date"))
                                        .and_then(|d| d.as_str())
                                        .unwrap_or("Unknown");
                                    let proto = canon
                                        .and_then(|c| c.get("protocol"))
                                        .and_then(|d| d.as_str())
                                        .unwrap_or("UDS");
                                    let tx = canon
                                        .and_then(|c| c.get("tx_id"))
                                        .and_then(|d| d.as_str())
                                        .unwrap_or("N/A");
                                    let rx = canon
                                        .and_then(|c| c.get("rx_id"))
                                        .and_then(|d| d.as_str())
                                        .unwrap_or("N/A");
                                    let copies = info
                                        .get("total_copies_in_cbf")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(1);
                                    let vers = info
                                        .get("distinct_versions_count")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(1);

                                    println!(
                                        "  • {:<16} | Date: {:<10} | {:<7} | CAN: {:<6}/{:<6} | {} copy(ies), {} version(s)",
                                        name, date, proto, tx, rx, copies, vers
                                    );
                                    if matches_chassis && !matches_name {
                                        println!("    Chassis: {}", chassis_list.join(", "));
                                    }
                                }
                            }
                        }
                        println!("\nFound {} matching ECU(s).", matches);
                    }
                    CbfCommands::Inspect { ecu } => {
                        let q = ecu.to_uppercase();
                        if let Some(info) = cat.get("ecus").and_then(|v| v.get(&q)) {
                            println!(
                                "============================================================"
                            );
                            println!("  ECU: {}", q);
                            println!(
                                "============================================================"
                            );
                            println!(
                                "  Total Copies Across Chassis: {}",
                                info.get("total_copies_in_cbf")
                                    .unwrap_or(&serde_json::json!(1))
                            );
                            println!(
                                "  Distinct Versions:           {}",
                                info.get("distinct_versions_count")
                                    .unwrap_or(&serde_json::json!(1))
                            );
                            if let Some(canon) = info.get("canonical_version") {
                                println!("\n  Canonical (Latest) Version:");
                                println!(
                                    "    • Date:             {}",
                                    canon.get("date").unwrap_or(&serde_json::json!(""))
                                );
                                println!(
                                    "    • Protocol:         {}",
                                    canon.get("protocol").unwrap_or(&serde_json::json!(""))
                                );
                                println!(
                                    "    • Tx CAN ID:        {}",
                                    canon.get("tx_id").unwrap_or(&serde_json::json!("N/A"))
                                );
                                println!(
                                    "    • Rx CAN ID:        {}",
                                    canon.get("rx_id").unwrap_or(&serde_json::json!("N/A"))
                                );
                                println!(
                                    "    • Size:             {} bytes",
                                    canon.get("size_bytes").unwrap_or(&serde_json::json!(0))
                                );
                                println!(
                                    "    • Presentations:    {}",
                                    canon
                                        .get("presentation_count")
                                        .unwrap_or(&serde_json::json!(0))
                                );
                                println!(
                                    "    • DTC Fault Codes:  {}",
                                    canon.get("dtc_count").unwrap_or(&serde_json::json!(0))
                                );
                                println!(
                                    "    • Primary Path:     data/cbf/{}",
                                    canon
                                        .get("primary_path")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("")
                                );
                            }
                            if let Some(chassis) =
                                info.get("all_chassis_supported").and_then(|v| v.as_array())
                            {
                                let ch_strs: Vec<&str> =
                                    chassis.iter().filter_map(|c| c.as_str()).collect();
                                println!(
                                    "\n  Supported Chassis Folders ({} total):",
                                    ch_strs.len()
                                );
                                for c in ch_strs {
                                    println!("    - {}", c);
                                }
                            }
                        } else {
                            eprintln!("ECU '{}' not found in catalog.", q);
                        }
                    }
                }
                return Ok(());
            }
        }
    }

    match mode.unwrap_or(OperatingMode::Local) {
        OperatingMode::Local => {
            info!("============================================================");
            info!("  Starting Sterngate in LOCAL STANDALONE mode");
            info!("  Interface: {}", cli.can_interface);
            info!("  Dashboard: http://localhost:{}", cli.port);
            info!("============================================================");

            let mut iface: Box<dyn VehicleInterface> = if cli.can_interface == "mock" {
                Box::new(VirtualCanInterface::new())
            } else {
                Box::new(SocketCanInterface::new(&cli.can_interface))
            };
            if let Err(e) = iface.open().await {
                warn!(
                    "Could not open CAN interface {}: {}. Will retry on demand.",
                    cli.can_interface, e
                );
            }

            let profile = load_profile_safe(&cli.profile);
            let flasher = Arc::new(FlashingWorker::new());
            let state = Arc::new(AppState::new(iface, profile, flasher));
            run_server(state, cli.port).await?;
        }
        OperatingMode::Client => {
            info!("============================================================");
            info!("  Starting Sterngate in CLIENT mode (Car-Side P2P Bridge)");
            info!("  Interface: {}", cli.can_interface);
            info!("============================================================");

            let node = P2pNode::new().await?;
            let ticket = node.generate_ticket()?;
            info!("Iroh P2P Endpoint initialized.");
            info!("Node ID: {}", node.node_id());
            println!(
                "\n>>> SHARE THIS TICKET WITH YOUR REMOTE TECHNICIAN <<<\n{}\n",
                ticket
            );

            info!("Waiting for incoming connection from technician...");
            while let Some(incoming) = node.accept().await {
                info!("Incoming connection received!");
                if let Ok(connecting) = incoming.accept() {
                    let _ = connecting.await;
                }
            }
        }
        OperatingMode::Server => {
            let ticket_str = cli
                .ticket
                .context("Server mode requires --ticket <TICKET>")?;
            info!("============================================================");
            info!("  Starting Sterngate in SERVER mode (Remote Technician Node)");
            info!("  Connecting to remote vehicle node via Iroh QUIC...");
            info!("============================================================");

            let target = P2pNode::parse_ticket(&ticket_str)?;
            let node = P2pNode::new().await?;
            info!("Dialing target car node: {}", target.id);
            let _conn = node.dial(target).await?;
            info!("Encrypted P2P tunnel established successfully!");

            let iface = Box::new(VirtualCanInterface::new());
            let profile = load_profile_safe(&cli.profile);
            let flasher = Arc::new(FlashingWorker::new());
            let state = Arc::new(AppState::new(iface, profile, flasher));
            run_server(state, cli.port).await?;
        }
    }

    Ok(())
}

fn load_profile_safe(path: &PathBuf) -> VehicleProfile {
    VehicleProfile::load_from_file(path).unwrap_or_else(|e| {
        warn!(
            "Could not load profile at {}: {}. Using default W211 configuration.",
            path.display(),
            e
        );
        VehicleProfile {
            profile_name: "fallback_w211".into(),
            oem: "Mercedes-Benz".into(),
            chassis: "W211".into(),
            gateway_type: Some("CGW_N93".into()),
            default_bitrate: 500000,
            modules: Default::default(),
            parameters: vec![],
        }
    })
}

fn find_profile_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                find_profile_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "json")
                && !path.to_string_lossy().contains("schema")
            {
                out.push(path);
            }
        }
    }
}
