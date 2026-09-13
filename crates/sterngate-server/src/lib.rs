pub mod dashboard;
pub mod routes;
pub mod state;
pub mod ws;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

pub use routes::create_router;
pub use state::AppState;

pub async fn run_server(state: Arc<AppState>, port: u16) -> Result<()> {
    let app = create_router(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Sterngate Web Dashboard listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
