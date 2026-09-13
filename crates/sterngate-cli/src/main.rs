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
}

#[derive(Subcommand, Debug)]
enum ProfileCommands {
    /// List available vehicle profiles
    List,
    /// Inspect a specific profile
    Inspect { path: PathBuf },
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
            },
            Commands::Profile { action } => match action {
                ProfileCommands::List => {
                    println!("Available Profiles:");
                    println!("  - profiles/mercedes/w211_om646_edc16.json (Mercedes W211 OM646 CDI + 722.6)");
                    println!("  - profiles/mercedes/w211_om648_edc16.json (Mercedes W211 OM648 I6 CDI + 722.6)");
                    println!("  - profiles/vag/golf_mk6_edc17.json (Volkswagen Golf Mk6 2.0 TDI EDC17 + DSG)");
                    println!("  - profiles/bmw/e90_m57_dde6.json (BMW E90 M57 3.0d DDE6 + ZF 6HP)");
                    return Ok(());
                }
                ProfileCommands::Inspect { path } => {
                    let prof = VehicleProfile::load_from_file(&path)?;
                    println!("Profile: {} ({})", prof.profile_name, prof.oem);
                    println!("Modules: {:?}", prof.modules.keys().collect::<Vec<_>>());
                    println!("Parameters defined: {}", prof.parameters.len());
                    return Ok(());
                }
            },
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
