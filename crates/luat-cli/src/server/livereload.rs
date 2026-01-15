// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! WebSocket handler for live reload functionality.

use axum::extract::ws::{Message, WebSocket};
use tokio::sync::broadcast;

/// Handles a WebSocket connection for live reload notifications.
pub async fn handle_websocket(mut socket: WebSocket, mut rx: broadcast::Receiver<()>) {
    loop {
        tokio::select! {
            // Wait for reload signal
            result = rx.recv() => {
                match result {
                    Ok(()) => {
                        // Send reload message to client
                        if socket.send(Message::Text("reload".to_string())).await.is_err() {
                            // Client disconnected
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Channel closed
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // We lagged behind, just continue
                        continue;
                    }
                }
            }
            // Handle incoming messages from client (keep-alive, etc.)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => {
                        // Client disconnected
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        // Respond to ping with pong
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(_)) => {
                        // Ignore other messages
                    }
                    Some(Err(_)) => {
                        // Error receiving
                        break;
                    }
                }
            }
        }
    }
}
