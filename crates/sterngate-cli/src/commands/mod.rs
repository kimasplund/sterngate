use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use crate::args::{Cli, Commands};
use sterngate_hal::{VehicleInterface, VirtualCanInterface};
use sterngate_mcp::McpServer;
use sterngate_protocol::FlashingWorker;
use sterngate_server::{run_server, AppState};

pub mod analyze;
pub mod coding;
pub mod common;
pub mod diag;
pub mod ecu;
pub mod flash;
pub mod modcmd;
pub mod profile;
pub mod service;
pub mod tune;
pub mod vehicle;

pub async fn handle_command(cmd: Commands, cli: &Cli) -> Result<()> {
    match cmd {
        Commands::Mcp => {
            let mcp = McpServer::new();
            mcp.run_stdio().await?;
        }
        Commands::Mock { port } => {
            info!(
                "Starting Sterngate in MOCK SIMULATION mode on port {}",
                port
            );
            let mut iface = Box::new(VirtualCanInterface::new());
            let _ = iface.open().await;
            let profile = common::load_profile_safe(&cli.profile);
            let flasher = Arc::new(FlashingWorker::new());
            let state =
                Arc::new(AppState::new(iface, profile, flasher).with_vault_root(cli.vault.clone()));
            run_server(state, cli.bind, port).await?;
        }
        Commands::Diag { action } => diag::execute(action, cli).await?,
        Commands::Flash { action } => flash::execute(action, cli).await?,
        Commands::Service { action } => service::execute(action, cli).await?,
        Commands::Coding { action } => coding::execute(action, cli).await?,
        Commands::Vehicle { action } => vehicle::execute(action)?,
        Commands::Analyze { action } => analyze::execute(action).await?,
        Commands::Profile { action } => profile::execute(action, cli).await?,
        Commands::Ecu { action } => ecu::execute(action)?,
        Commands::Mod { action } => modcmd::execute(action, cli).await?,
        Commands::Tune { action } => tune::execute(action)?,
    }
    Ok(())
}
