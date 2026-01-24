use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::Utc;
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Controller,
    Doer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoomRequest {
    pub role: Role,
    pub max_controllers: usize,
    pub max_doers: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomListResponse {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub controllers: usize,
    pub max_controllers: usize,
    pub doers: usize,
    pub max_doers: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
#[serde(rename_all = "camelCase")]
pub enum WsMessage {
    Chat { sender: String, text: String },
    Signal { color: String },
    Status { text: String },
    Identity { role: Role, username: String, current_color: String },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub struct UserSession {
    pub role: Role,
    pub username: String,
}

pub struct Room {
    pub id: String,
    pub label: String,
    pub tx: broadcast::Sender<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub users: HashMap<String, UserSession>,
    pub max_controllers: usize,
    pub max_doers: usize,
    pub current_color: String,
}

pub struct InteractionState {
    pub rooms: RwLock<HashMap<String, Room>>,
}

impl InteractionState {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
        }
    }
}

pub async fn create_room(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateRoomRequest>,
) -> impl IntoResponse {
    let room_id = Uuid::new_v4().to_string();
    let (tx, _rx) = broadcast::channel(100);

    let label = match payload.role {
        Role::Controller => "Controller".to_string(),
        Role::Doer => "Doer".to_string(),
    };

    let room = Room {
        id: room_id.clone(),
        label,
        tx,
        created_at: Utc::now(),
        users: HashMap::new(),
        max_controllers: payload.max_controllers,
        max_doers: payload.max_doers,
        current_color: "#808080".to_string(), // Default gray
    };

    state.interaction.rooms.write().await.insert(room_id.clone(), room);

    Json(json!({ "room_id": room_id }))
}

pub async fn list_rooms(
    State(state): State<Arc<AppState>>,
    Path(role_str): Path<String>,
) -> impl IntoResponse {
    let desired_role = match role_str.to_lowercase().as_str() {
        "controller" => Role::Controller,
        "doer" => Role::Doer,
        _ => return (StatusCode::BAD_REQUEST, "Invalid role").into_response(),
    };

    let rooms = state.interaction.rooms.read().await;
    let mut available_rooms = Vec::new();

    for room in rooms.values() {
        let controller_count = room.users.values().filter(|u| u.role == Role::Controller).count();
        let doer_count = room.users.values().filter(|u| u.role == Role::Doer).count();

        let is_available = match desired_role {
            Role::Controller => controller_count < room.max_controllers,
            Role::Doer => doer_count < room.max_doers,
        };

        if is_available {
            available_rooms.push(RoomListResponse {
                id: room.id.clone(),
                label: room.label.clone(),
                created_at: room.created_at.to_rfc3339(),
                controllers: controller_count,
                max_controllers: room.max_controllers,
                doers: doer_count,
                max_doers: room.max_doers,
            });
        }
    }

    available_rooms.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Json(available_rooms).into_response()
}

#[derive(Deserialize)]
pub struct WsParams {
    role: String,
    username: String,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    Query(params): Query<WsParams>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let role = match params.role.to_lowercase().as_str() {
        "controller" => Role::Controller,
        "doer" => Role::Doer,
        _ => return (StatusCode::BAD_REQUEST, "Invalid role").into_response(),
    };
    
    // Simple username validation
    let username = params.username.trim().to_string();
    if username.is_empty() {
         return (StatusCode::BAD_REQUEST, "Username required").into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(socket, room_id, role, username, state))
}

async fn handle_socket(socket: WebSocket, room_id: String, role: Role, username: String, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let connection_id = Uuid::new_v4().to_string();
    let mut rx;

    // Join Room Block
    {
        let mut rooms = state.interaction.rooms.write().await;
        if let Some(room) = rooms.get_mut(&room_id) {
            // Check capacity
            let count = room.users.values().filter(|u| u.role == role).count();
            let max = match role {
                Role::Controller => room.max_controllers,
                Role::Doer => room.max_doers,
            };

            if count >= max {
                let _ = sender.send(Message::Text(serde_json::to_string(&WsMessage::Error {
                    message: "Room is full for this role.".to_string()
                }).unwrap().into())).await;
                return;
            }

            room.users.insert(connection_id.clone(), UserSession {
                role: role.clone(),
                username: username.clone(),
            });
            
            rx = room.tx.subscribe();

            // Send Identity with current color state
            let _ = sender.send(Message::Text(serde_json::to_string(&WsMessage::Identity {
                role: role.clone(),
                username: username.clone(),
                current_color: room.current_color.clone(),
            }).unwrap().into())).await;
            
            // Notify others
            let _ = room.tx.send(serde_json::to_string(&WsMessage::Status {
                text: format!("{} ({:?}) joined the room.", username, role)
            }).unwrap());

        } else {
             let _ = sender.send(Message::Text(serde_json::to_string(&WsMessage::Error {
                message: "Room not found.".to_string()
            }).unwrap().into())).await;
            return;
        }
    }

    // Forward broadcast messages to this client
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages from this client
    let tx_inner_state = state.clone();
    let room_id_inner = room_id.clone();
    let username_inner = username.clone();
    
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                let text_string = text.to_string();
                
                // Try parsing as JSON first (for signals)
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text_string) {
                    if let Some(msg_type) = parsed.get("type").and_then(|v| v.as_str()) {
                         if msg_type == "signal" {
                             if let Some(payload) = parsed.get("payload") {
                                 if let Some(color) = payload.get("color").and_then(|v| v.as_str()) {
                                     // Update State
                                     let mut rooms = tx_inner_state.interaction.rooms.write().await;
                                     if let Some(room) = rooms.get_mut(&room_id_inner) {
                                         room.current_color = color.to_string();
                                         let _ = room.tx.send(text_string.clone());
                                     }
                                 }
                             }
                         } else {
                             // Re-broadcast other JSON messages
                             let rooms = tx_inner_state.interaction.rooms.read().await;
                             if let Some(room) = rooms.get(&room_id_inner) {
                                  let _ = room.tx.send(text_string);
                             }
                         }
                    }
                } else {
                    // Plain text is chat
                    let rooms = tx_inner_state.interaction.rooms.read().await;
                    if let Some(room) = rooms.get(&room_id_inner) {
                        let chat_msg = WsMessage::Chat {
                            sender: username_inner.clone(),
                            text: text_string,
                        };
                        let _ = room.tx.send(serde_json::to_string(&chat_msg).unwrap());
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    // Cleanup
    let mut rooms = state.interaction.rooms.write().await;
    if let Some(room) = rooms.get_mut(&room_id) {
        room.users.remove(&connection_id);
        let _ = room.tx.send(serde_json::to_string(&WsMessage::Status {
            text: format!("{} left the room.", username)
        }).unwrap());
        
        if room.users.is_empty() {
             rooms.remove(&room_id);
        }
    }
}
