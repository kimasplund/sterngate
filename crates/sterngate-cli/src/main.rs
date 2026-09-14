use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod args;
mod commands;

use args::{Cli, OperatingMode};
use commands::common::load_profile_safe;
use sterngate_hal::{OpenPortInterface, SocketCanInterface, VehicleInterface, VirtualCanInterface};
use sterngate_p2p::P2pNode;
use sterngate_protocol::FlashingWorker;
use sterngate_server::{run_server, AppState};

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

    let mut cli = Cli::parse();
    if cli.openport {
        cli.can_interface = "openport".to_string();
    }

    // Determine operational mode
    let mode = if cli.local {
        Some(OperatingMode::Local)
    } else if cli.bridge {
        Some(OperatingMode::Bridge)
    } else if cli.tech || (cli.ticket.is_some() && cli.mode.is_none()) {
        Some(OperatingMode::Tech)
    } else {
        cli.mode
    };

    if let Some(cmd) = cli.command.take() {
        commands::handle_command(cmd, &cli).await?;
        return Ok(());
    }

    match mode.unwrap_or(OperatingMode::Local) {
        OperatingMode::Local => {
            info!("============================================================");
            info!("  Starting Sterngate in LOCAL STANDALONE mode");
            info!("  Interface: {}", cli.can_interface);
            info!("  Dashboard: http://localhost:{}", cli.port);
            info!("  Notice: Use at your own risk. See DISCLAIMER.md.");
            info!("============================================================");

            let mut iface: Box<dyn VehicleInterface> = if cli.can_interface == "mock" {
                Box::new(VirtualCanInterface::new())
            } else if cli.can_interface == "openport" || cli.can_interface == "tactrix" {
                Box::new(OpenPortInterface::new())
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
        OperatingMode::Bridge => {
            info!("============================================================");
            info!("  Starting Sterngate: Car-Side Diagnostic Bridge (P2P Host)");
            info!("  Role: On-Vehicle Gateway Bridge & Ticket Host");
            info!("  Interface: {}", cli.can_interface);
            info!("  Notice: Use at your own risk. See DISCLAIMER.md.");
            info!("============================================================");

            let node = P2pNode::new().await?;
            let ticket = node.generate_ticket()?;
            info!("Iroh P2P Endpoint initialized.");
            info!("Node ID: {}", node.node_id());
            println!(
                "\n>>> SHARE THIS TICKET WITH YOUR REMOTE TECHNICIAN <<<\n{}\n",
                ticket
            );

            info!("Waiting for incoming connection from remote technician client...");
            while let Some(incoming) = node.accept().await {
                info!("Incoming connection received!");
                if let Ok(connecting) = incoming.accept() {
                    let _ = connecting.await;
                }
            }
        }
        OperatingMode::Tech => {
            let ticket_str = cli
                .ticket
                .context("Technician mode requires --ticket <TICKET>")?;
            info!("============================================================");
            info!("  Starting Sterngate: Remote Technician Client (P2P Dialer)");
            info!("  Role: Remote Technician Console (Tester)");
            info!("  Connecting to car-side diagnostic bridge via Iroh QUIC...");
            info!("  Technician Dashboard: http://localhost:{}", cli.port);
            info!("  Notice: Use at your own risk. See DISCLAIMER.md.");
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
