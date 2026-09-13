use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tracing::{debug, info};

pub async fn ws_telemetry_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.telemetry_tx.subscribe();

    info!("WebSocket client connected for real-time telemetry");

    let send_task = tokio::spawn(async move {
        while let Ok(snapshot) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&snapshot) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    tokio::select! {
        _ = send_task => debug!("WS send task finished"),
        _ = recv_task => debug!("WS receive task finished"),
    }

    info!("WebSocket client disconnected");
}
