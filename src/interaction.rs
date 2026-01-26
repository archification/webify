use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State, Form, Multipart,
    },
    http::StatusCode,
    response::{Html, IntoResponse},
};

use base64::{Engine as _, engine::general_purpose};
use chrono::Utc;
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
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

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Controller => write!(f, "Controller"),
            Role::Doer => write!(f, "Doer"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomForm {
    pub username: String,
    pub role: String,
    pub max_controllers: usize,
    pub max_doers: usize,
}

#[derive(Debug, Deserialize)]
pub struct JoinRoomForm {
    pub username: String,
    pub role: String,
    pub room_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct WsParams {
    role: String,
    username: String,
}

#[derive(Deserialize)]
struct HtmxWsMessage {
    #[serde(default)]
    chat_message: String,
    #[serde(default)]
    signal: String,
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

fn render_room_view(room_id: &str, role: Role, username: &str, current_color: &str) -> String {
    let ws_url = format!("/ws/interaction/{}?role={:?}&username={}", 
        room_id, role, urlencoding::encode(username));
    let controller_ui = if role == Role::Controller {
        let btn_class = "color-btn";
        let active = |c: &str| if current_color == c { "active" } else { "" };
        format!(r###"
            <div id="view-controller" style="text-align: center; margin-bottom: 20px;">
                <form hx-ws="send" style="display: inline-block;">
                    <button class="{btn_class} btn-red {a_red}" name="signal" value="#dc322f">Red</button>
                    <button class="{btn_class} btn-green {a_green}" name="signal" value="#859900">Green</button>
                    <button class="{btn_class} btn-blue {a_blue}" name="signal" value="#268bd2">Blue</button>
                    <button class="{btn_class} btn-yellow {a_yellow}" name="signal" value="#b58900">Yellow</button>
                </form>
            </div>
        "###, 
        btn_class=btn_class,
        a_red=active("#dc322f"),
        a_green=active("#859900"),
        a_blue=active("#268bd2"),
        a_yellow=active("#b58900")
        )
    } else {
        String::new()
    };
    let doer_ui = if role == Role::Doer {
        format!(r###"
            <div id="view-doer" style="margin-bottom: 20px; text-align: center;">
                <div id="signal-circle" style="width: 200px; height: 200px; border-radius: 50%; background-color: {}; margin: 0 auto; transition: background-color 0.3s;"></div>
                <p>Watch the circle!</p>
            </div>
        "###, current_color)
    } else {
        String::new()
    };
    format!(r###"
        <div id="room-container" hx-ext="ws" ws-connect="{}">
            <div style="display: flex; justify-content: space-between; align-items: center; border-bottom: 1px solid #586e75; padding-bottom: 10px; margin-bottom: 20px;">
                <h2>Room: {} ({})</h2>
                <button hx-get="/interaction" hx-target="body" style="background-color: #dc322f; color: white; border: none; padding: 5px 10px; cursor: pointer;">Leave</button>
            </div>

            {}
            {}

            <h3>Chat</h3>
            <div id="chat-container" style="height: 400px; overflow-y: scroll; border: 1px solid #586e75; padding: 10px; background: #002b36; margin-bottom: 10px;">
                <div class="system-msg" style="color: #b58900; font-style: italic;">Connected as {}.</div>
            </div>

            <div style="display: flex; gap: 10px;">
                <form hx-ws="send" hx-on:htmx:ws-after-send="this.reset()" style="flex-grow: 1; display: flex; gap: 5px;">
                    <input type="text" name="chat_message" placeholder="Type a message..." style="flex-grow: 1; padding: 10px;" required autocomplete="off">
                    <button type="submit" style="padding: 10px;">Send</button>
                </form>

                <form hx-post="/interaction/upload/{}" hx-encoding="multipart/form-data" hx-target="#upload-status" hx-on:htmx:after-request="this.reset()" style="display: flex; align-items: center;">
                    <input type="hidden" name="username" value="{}">
                    <label for="file-upload" style="cursor: pointer; padding: 10px; font-size: 1.5em;" title="Upload File">📎</label>
                    <input id="file-upload" type="file" name="file" style="display: none;" onchange="this.form.requestSubmit()">
                    <span id="upload-status" style="font-size: 0.8em; margin-left: 5px;"></span>
                </form>
            </div>
        </div>
    "###, ws_url, role, username, doer_ui, controller_ui, username, room_id, username)
}

pub async fn create_room(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateRoomForm>,
) -> impl IntoResponse {
    let room_id = Uuid::new_v4().to_string();
    let (tx, _rx) = broadcast::channel(100);
    let user_role = match form.role.to_lowercase().as_str() {
        "controller" => Role::Controller,
        _ => Role::Doer,
    };
    let label = match user_role {
        Role::Controller => "Controller Room",
        Role::Doer => "Doer Room",
    };
    let default_color = "#808080".to_string();
    let room = Room {
        id: room_id.clone(),
        label: label.to_string(),
        tx,
        created_at: Utc::now(),
        users: HashMap::new(),
        max_controllers: form.max_controllers,
        max_doers: form.max_doers,
        current_color: default_color.clone(),
    };
    state.interaction.rooms.write().await.insert(room_id.clone(), room);
    Html(render_room_view(&room_id, user_role, &form.username, &default_color))
}

pub async fn join_room(
    State(state): State<Arc<AppState>>,
    Form(form): Form<JoinRoomForm>,
) -> impl IntoResponse {
    let rooms = state.interaction.rooms.read().await;
    if let Some(room) = rooms.get(&form.room_id) {
        let role = match form.role.as_str() {
            "controller" => Role::Controller,
            _ => Role::Doer,
        };
        let count = room.users.values().filter(|u| u.role == role).count();
        let max = if role == Role::Controller { room.max_controllers } else { room.max_doers };
        if count >= max {
            return Html(format!("<div class='error'>Room is full for {:?}. <button hx-get='/interaction' hx-target='body'>Back</button></div>", role));
        }
        Html(render_room_view(&room.id, role, &form.username, &room.current_color))
    } else {
        Html("<div class='error'>Room not found. <button hx-get='/interaction' hx-target='body'>Back</button></div>".to_string())
    }
}

pub async fn list_rooms(
    State(state): State<Arc<AppState>>,
    Path(role_str): Path<String>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let desired_role = match role_str.to_lowercase().as_str() {
        "controller" => Role::Controller,
        "doer" => Role::Doer,
        _ => return Html("<p>Invalid role selected</p>".to_string()),
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
            available_rooms.push((room, controller_count, doer_count));
        }
    }
    available_rooms.sort_by(|(a, _, _), (b, _, _)| b.created_at.cmp(&a.created_at));
    if available_rooms.is_empty() {
        return Html("<p>No rooms available.</p>".to_string());
    }
    let mut html = String::new();
    html.push_str("<div class='room-list'>");
    for (room, c_count, d_count) in available_rooms {
        let role_val = match desired_role { Role::Controller => "controller", Role::Doer => "doer" };
        let info = if desired_role == Role::Controller {
            format!("Controllers: {}/{}", c_count, room.max_controllers)
        } else {
            format!("Doers: {}/{}", d_count, room.max_doers)
        };
        html.push_str(&format!(r###"
            <div class="room-list-item" style="border: 1px solid #586e75; padding: 10px; margin-bottom: 5px;">
                <form hx-post="/interaction/join" hx-target="#main-container">
                    <input type="hidden" name="username" value="{}">
                    <input type="hidden" name="role" value="{}">
                    <input type="hidden" name="room_id" value="{}">
                    <div style="display: flex; justify-content: space-between; align-items: center;">
                        <div>
                            <strong>{}</strong> <small>({})</small><br>
                            <span style="font-size: 0.8em; color: #93a1a1;">{}</span>
                        </div>
                        <button type="submit">Join</button>
                    </div>
                </form>
            </div>
        "###, 
        params.username, 
        role_val, 
        room.id, 
        room.label, 
        info, 
        room.created_at.format("%H:%M:%S")
        ));
    }
    html.push_str("</div>");
    Html(html)
}

pub async fn upload_file(
    Path(room_id): Path<String>,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut username = String::from("Anonymous");
    let mut file_name = String::new();
    let mut file_content = Vec::new();
    let mut content_type = String::from("application/octet-stream");
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let name = field.name().unwrap_or_default().to_string();
                if name == "username" {
                    if let Ok(text) = field.text().await {
                        username = text;
                    }
                } else if name == "file" {
                    file_name = field.file_name().unwrap_or_default().to_string();
                    file_name = sanitize_filename::sanitize(&file_name); 
                    if let Some(ct) = field.content_type() {
                        content_type = ct.to_string();
                    }
                    match field.bytes().await {
                        Ok(bytes) => {
                             file_content = bytes.to_vec();
                        }
                        Err(e) => {
                             return Html(format!("<span style='color: red;'>Read Error: {}</span>", e));
                        }
                    }
                }
            },
            Ok(None) => break,
            Err(e) => {
                 return Html(format!("<span style='color: red;'>Upload Error: {}</span>", e));
            }
        }
    }
    if file_content.is_empty() || file_name.is_empty() {
        return Html("<span style='color: red;'>No file received or empty file.</span>".to_string());
    }
    let b64 = general_purpose::STANDARD.encode(&file_content);
    let data_uri = format!("data:{};base64,{}", content_type, b64);
    let content_html = if content_type.starts_with("image/") {
        format!(r###"<br><img src="{}" alt="{}" style="max-width: 100%; max-height: 300px; border-radius: 5px; margin-top: 5px;">"###, 
            data_uri, file_name)
    } else if content_type.starts_with("video/") {
        format!(r###"<br><video controls src="{}" style="max-width: 100%; max-height: 300px; margin-top: 5px;"></video>"###, 
            data_uri)
    } else if content_type.starts_with("audio/") {
        format!(r###"<br><audio controls src="{}" style="margin-top: 5px;"></audio>"###, 
            data_uri)
    } else {
        format!(r###"Shared file: <a href="{}" download="{}" style="color: #268bd2;">{}</a>"###, 
            data_uri, file_name, file_name)
    };
    let rooms = state.interaction.rooms.read().await;
    if let Some(room) = rooms.get(&room_id) {
         let msg = format!(r###"<div hx-swap-oob="beforeend:#chat-container">
                <div class="message">
                    <span class="sender" style="font-weight: bold; color: #268bd2;">{}: </span>
                    <span>{}</span>
                </div>
               </div>"###, username, content_html);
         let _ = room.tx.send(msg);
    }
    Html("<span style='color: #859900;'>Uploaded!</span>".to_string())
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    Query(params): Query<WsParams>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let role = match params.role.to_lowercase().as_str() {
        "controller" => Role::Controller,
        _ => Role::Doer,
    };
    let username = params.username.trim().to_string();
    if username.is_empty() {
         return (StatusCode::BAD_REQUEST, "Username required").into_response();
    }
    ws.max_message_size(128 * 1024 * 1024)
      .max_frame_size(128 * 1024 * 1024)
      .on_upgrade(move |socket| handle_socket(socket, room_id, role, username, state))
}

async fn handle_socket(socket: WebSocket, room_id: String, role: Role, username: String, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let connection_id = Uuid::new_v4().to_string();
    let mut rx;
    {
        let mut rooms = state.interaction.rooms.write().await;
        if let Some(room) = rooms.get_mut(&room_id) {
            room.users.insert(connection_id.clone(), UserSession {
                role: role.clone(),
                username: username.clone(),
            });
            rx = room.tx.subscribe();
            let join_msg = format!(
                r###"<div hx-swap-oob="beforeend:#chat-container"><div class="system-msg" style="color: #b58900; font-style: italic;">{} ({}) joined.</div></div>"###,
                username, role
            );
            let _ = room.tx.send(join_msg);
        } else {
            return;
        }
    }
    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if sender.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                },
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    continue;
                },
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });
    let tx_inner_state = state.clone();
    let room_id_inner = room_id.clone();
    let username_inner = username.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(payload) = serde_json::from_str::<HtmxWsMessage>(&text) {
                    let mut rooms = tx_inner_state.interaction.rooms.write().await;
                    if let Some(room) = rooms.get_mut(&room_id_inner) {
                        if !payload.chat_message.is_empty() {
                            let safe_msg = payload.chat_message.replace("<", "&lt;").replace(">", "&gt;");
                            let mut formatted_msg = safe_msg.clone();
                            if safe_msg.starts_with("http") {
                                formatted_msg = format!(r###"<a href="{}" target="_blank" style="color: #268bd2;">{}</a>"###, safe_msg, safe_msg);
                            }
                            let html = format!(
                                r###"<div hx-swap-oob="beforeend:#chat-container">
                                    <div class="message">
                                        <span class="sender" style="font-weight: bold; color: #268bd2;">{}: </span>
                                        <span>{}</span>
                                    </div>
                                   </div>"###, 
                                username_inner, formatted_msg
                            );
                            let _ = room.tx.send(html);
                        }
                        if !payload.signal.is_empty() {
                            room.current_color = payload.signal.clone();
                            let doer_html = format!(
                                r###"<div id="signal-circle" style="width: 200px; height: 200px; border-radius: 50%; background-color: {}; margin: 0 auto; transition: background-color 0.3s;" hx-swap-oob="true"></div>"###,
                                payload.signal
                            );
                            let _ = room.tx.send(doer_html);
                            let btn_class = "color-btn";
                            let active = |c: &str| if payload.signal == c { "active" } else { "" };
                            let controller_html = format!(r###"
                                <div id="view-controller" style="text-align: center; margin-bottom: 20px;" hx-swap-oob="true">
                                    <form hx-ws="send" style="display: inline-block;">
                                        <button class="{btn_class} btn-red {a_red}" name="signal" value="#dc322f">Red</button>
                                        <button class="{btn_class} btn-green {a_green}" name="signal" value="#859900">Green</button>
                                        <button class="{btn_class} btn-blue {a_blue}" name="signal" value="#268bd2">Blue</button>
                                        <button class="{btn_class} btn-yellow {a_yellow}" name="signal" value="#b58900">Yellow</button>
                                    </form>
                                </div>
                            "###, 
                            btn_class=btn_class,
                            a_red=active("#dc322f"),
                            a_green=active("#859900"),
                            a_blue=active("#268bd2"),
                            a_yellow=active("#b58900")
                            );
                            let _ = room.tx.send(controller_html);
                        }
                    }
                }
            }
        }
    });
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
    let mut rooms = state.interaction.rooms.write().await;
    if let Some(room) = rooms.get_mut(&room_id) {
        room.users.remove(&connection_id);
        let leave_msg = format!(
            r###"<div hx-swap-oob="beforeend:#chat-container"><div class="system-msg" style="color: #b58900; font-style: italic;">{} left.</div></div>"###,
            username
        );
        let _ = room.tx.send(leave_msg);
        
        if room.users.is_empty() {
            rooms.remove(&room_id);
        }
    }
}
