use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State, Form, Multipart,
    },
    http::StatusCode,
    response::{Html, IntoResponse},
};

use chrono::Utc;
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;
use std::pin::Pin;
use std::future::Future;

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

pub type CommandFn = Box<dyn Fn(Vec<String>, broadcast::Sender<String>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

#[derive(Debug, Deserialize)]
pub struct CreateRoomForm {
    pub username: String,
    pub role: String,
    pub max_controllers: usize,
    pub max_doers: usize,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JoinRoomForm {
    pub username: String,
    pub role: String,
    pub room_id: String,
    pub password: Option<String>,
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
    #[serde(default)]
    webrtc_signal: String,
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
    pub password: Option<String>,
    /// Filenames (relative to ./uploads) shared in this room's chat. Deleted from
    /// disk when the room is torn down — see the cleanup in `handle_socket`.
    pub uploaded_files: Vec<String>,
}

pub struct InteractionState {
    pub rooms: RwLock<HashMap<String, Room>>,
    pub commands: HashMap<String, CommandFn>,
}

impl InteractionState {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            commands: HashMap::new(),
        }
    }
    pub fn register_command<F, Fut>(&mut self, name: &str, handler: F)
    where
        F: Fn(Vec<String>, broadcast::Sender<String>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.commands.insert(name.to_string(), Box::new(move |args, tx| {
            Box::pin(handler(args, tx))
        }));
    }
}

fn render_room_view(room_id: &str, role: Role, username: &str, current_color: &str) -> String {
    let ws_url = format!("/ws/interaction/{}?role={:?}&username={}",
        room_id, role, urlencoding::encode(username));

    let controller_ui = if role == Role::Controller {
        let btn_class = "signal-btn";
        let active = |c: &str| if current_color == c { "active" } else { "" };
        format!(r###"
            <div id="view-controller" class="signal-panel">
                <form hx-ws="send" class="signal-btn-row">
                    <button class="{btn_class} sig-red {a_red}" name="signal" value="#ff3d6b">Red</button>
                    <button class="{btn_class} sig-green {a_green}" name="signal" value="#00ff41">Green</button>
                    <button class="{btn_class} sig-blue {a_blue}" name="signal" value="#00e5ff">Blue</button>
                    <button class="{btn_class} sig-yellow {a_yellow}" name="signal" value="#ffb347">Yellow</button>
                </form>
            </div>
        "###,
        btn_class=btn_class,
        a_red=active("#ff3d6b"),
        a_green=active("#00ff41"),
        a_blue=active("#00e5ff"),
        a_yellow=active("#ffb347")
        )
    } else {
        String::new()
    };

    let doer_ui = if role == Role::Doer {
        format!(r###"
            <div id="view-doer" class="signal-panel">
                <div id="signal-circle" class="signal-circle" style="background-color: {};"></div>
                <p class="signal-watch-label">Watch the circle!</p>
            </div>
        "###, current_color)
    } else {
        String::new()
    };

    format!(r###"
        <div id="room-container" hx-ext="ws" ws-connect="{}">
            <div class="room-view-header">
                <h2 class="room-view-title">Room: {} <span class="role-tag">// {}</span></h2>
                <button hx-get="/interaction" hx-target="body" class="btn-cyber btn-p btn-sm">Leave</button>
            </div>

            {}
            {}

            <div class="chat-label">Chat</div>
            <div id="chat-container" class="chat-container">
                <div class="chat-system">Connected as {}.</div>
            </div>

            <div class="chat-input-row">
                <form hx-ws="send" hx-on:htmx:ws-after-send="this.reset()" class="chat-input-row" style="flex-grow: 1;">
                    <input type="text" name="chat_message" placeholder="Type a message or /command..." class="cyber-input chat-text-input" required autocomplete="off">
                    <button type="submit" class="btn-cyber btn-c">Send</button>
                </form>

                <form hx-post="/interaction/upload/{}" hx-encoding="multipart/form-data" hx-target="#upload-status" hx-on:htmx:after-request="this.reset()" class="upload-form">
                    <input type="hidden" name="username" value="{}">
                    <label for="file-upload" class="upload-label" title="Upload File">📎</label>
                    <input id="file-upload" type="file" name="file" style="display: none;" onchange="this.form.requestSubmit()">
                    <span id="upload-status" class="upload-status"></span>
                </form>
            </div>

            <div class="voice-panel">
                <button id="mute-btn" class="btn-cyber btn-c">
                    🎤 Mute Microphone
                </button>
                <span id="voice-status" class="voice-status">Voice Chat Active</span>

                <div id="remote-audio-container"></div>

                <form id="webrtc-form" hx-ws="send" style="display: none;" onsubmit="event.preventDefault();">
                    <input type="hidden" id="webrtc-signal-input" name="webrtc_signal" value="">
                    <button type="submit" id="webrtc-submit-btn"></button>
                </form>
            </div>

            <script>
                (function() {{
                    var chatContainer = document.getElementById("chat-container");
                    var isScrolledToBottom = true;

                    chatContainer.addEventListener("scroll", function() {{
                        isScrolledToBottom = (chatContainer.scrollHeight - chatContainer.scrollTop - chatContainer.clientHeight) <= 10;
                    }});

                    var observer = new MutationObserver(function(mutations) {{
                        var shouldScroll = isScrolledToBottom;

                        if (shouldScroll) {{
                            requestAnimationFrame(() => {{
                                chatContainer.scrollTop = chatContainer.scrollHeight;
                            }});
                            
                            mutations.forEach(function(mutation) {{
                                mutation.addedNodes.forEach(function(node) {{
                                    if (node.nodeType === 1) {{
                                        var imgs = node.getElementsByTagName("img");
                                        for (var i = 0; i < imgs.length; i++) {{
                                            var img = imgs[i];
                                            var forceScroll = function() {{
                                                chatContainer.scrollTop = chatContainer.scrollHeight;
                                            }};

                                            if (img.complete) {{
                                                forceScroll();
                                            }} else {{
                                                img.addEventListener("load", forceScroll);
                                            }}
                                        }}
                                    }}
                                }});
                            }});
                        }}
                    }});
                    
                    observer.observe(chatContainer, {{ childList: true, subtree: true }});
                }})();

                // --- WebRTC Logic ---
                (async function() {{
                    const peerConnections = {{}};
                    let localStream;
                    const configuration = {{
                        'iceServers': [
                            {{'urls': 'stun:stun.l.google.com:19302'}},
                            {{
                                'urls': 'turn:openrelay.metered.ca:80',
                                'username': 'openrelayproject',
                                'credential': 'openrelayproject'
                            }},
                            {{
                                'urls': 'turn:openrelay.metered.ca:443',
                                'username': 'openrelayproject',
                                'credential': 'openrelayproject'
                            }},
                            {{
                                'urls': 'turn:openrelay.metered.ca:443?transport=tcp',
                                'username': 'openrelayproject',
                                'credential': 'openrelayproject'
                            }}
                        ]
                    }};
                    
                    try {{
                        localStream = await navigator.mediaDevices.getUserMedia({{ audio: true }});
                    }} catch (e) {{
                        document.getElementById('voice-status').innerText = "Mic Error";
                        document.getElementById('voice-status').style.color = "#dc322f";
                        return;
                    }}

                    let isMuted = false;
                    document.getElementById('mute-btn').addEventListener('click', function() {{
                        isMuted = !isMuted;
                        localStream.getAudioTracks().forEach(track => track.enabled = !isMuted);
                        this.innerText = isMuted ? "🔇 Unmute Microphone" : "🎤 Mute Microphone";
                        this.style.background = isMuted ? "#dc322f" : "#268bd2";
                    }});

                    window.sendWebRTCSignal = function(target_id, type, data = {{}}) {{
                        const payload = JSON.stringify({{ target: target_id, type: type, ...data }});
                        document.getElementById('webrtc-signal-input').value = payload;
                        // FIX 2: Triggering a button click natively bubbles the event safely to HTMX
                        document.getElementById('webrtc-submit-btn').click();
                    }};

                    async function handleWebRTCSignal(senderId, signal) {{
                        // FIX 3: Ignore your own broadcasts!
                        if (senderId === window.myConnectionId) return;
                        if (signal.target && signal.target !== window.myConnectionId) return;
                        
                        if (signal.type === "join") {{
                            const pc = createPeerConnection(senderId);
                            const offer = await pc.createOffer();
                            await pc.setLocalDescription(offer);
                            sendWebRTCSignal(senderId, "offer", {{ sdp: offer }});
                        }} 
                        else if (signal.type === "offer") {{
                            const pc = createPeerConnection(senderId);
                            await pc.setRemoteDescription(new RTCSessionDescription(signal.sdp));
                            const answer = await pc.createAnswer();
                            await pc.setLocalDescription(answer);
                            sendWebRTCSignal(senderId, "answer", {{ sdp: answer }});
                        }} 
                        else if (signal.type === "answer") {{
                            const pc = peerConnections[senderId];
                            if (pc) await pc.setRemoteDescription(new RTCSessionDescription(signal.sdp));
                        }} 
                        else if (signal.type === "ice") {{
                            const pc = peerConnections[senderId];
                            if (pc) await pc.addIceCandidate(new RTCIceCandidate(signal.candidate));
                        }}
                    }}

                    window.handleWebRTCSignalGlobal = handleWebRTCSignal;
                    
                    // Prevent memory leak of attaching multiple event listeners if user swaps rooms frequently
                    if (!window.webrtcListenerAttached) {{
                        document.body.addEventListener('htmx:wsBeforeMessage', async function(e) {{
                            if (typeof e.detail.message === 'string' && e.detail.message.startsWith('{{')) {{
                                e.preventDefault();
                                const data = JSON.parse(e.detail.message);
                                if (data.webrtc_welcome) {{
                                    window.myConnectionId = data.webrtc_welcome; // Catch the UUID assigned by the server
                                }} else if (data.webrtc && window.handleWebRTCSignalGlobal) {{
                                    await window.handleWebRTCSignalGlobal(data.sender_id, JSON.parse(data.webrtc));
                                }}
                            }}
                        }});
                        window.webrtcListenerAttached = true;
                    }}

                    function createPeerConnection(peerId) {{
                        const pc = new RTCPeerConnection(configuration);
                        peerConnections[peerId] = pc;

                        localStream.getTracks().forEach(track => pc.addTrack(track, localStream));

                        pc.onicecandidate = event => {{
                            if (event.candidate) {{
                                sendWebRTCSignal(peerId, "ice", {{ candidate: event.candidate }});
                            }}
                        }};

                        pc.ontrack = event => {{
                            let audio = document.getElementById(`audio-${{peerId}}`);
                            if (!audio) {{
                                audio = document.createElement('audio');
                                audio.id = `audio-${{peerId}}`;
                                audio.autoplay = true;
                                audio.controls = true;
                                audio.style.height = '30px';
                                audio.style.width = '100%';
                                document.getElementById('remote-audio-container').appendChild(audio);
                            }}
                            if (event.streams && event.streams[0]) {{
                                audio.srcObject = new MediaStream([event.track]);
                            }}
                            audio.play().catch(err => {{
                                console.error("Browser blocked autoplay. User must click play manually.", err);
                                document.getElementById('voice-status').innerText = "Autoplay Blocked - Click Play!";
                                document.getElementById('voice-status').style.color = "#b58900"; // Yellow warning
                            }});
                        }};
                        
                        pc.oniceconnectionstatechange = () => {{
                            if (pc.iceConnectionState === 'disconnected' || pc.iceConnectionState === 'failed') {{
                                const audio = document.getElementById(`audio-${{peerId}}`);
                                if (audio) audio.remove();
                                delete peerConnections[peerId];
                            }}
                        }};

                        return pc;
                    }}

                    setTimeout(() => sendWebRTCSignal(null, "join"), 500);
                }})();
            </script>
        </div>
    "###,
    ws_url,
    room_id,
    role,
    doer_ui,
    controller_ui,
    username,
    room_id,
    username
    )
}

fn render_password_prompt(room_id: &str, role: &str, username: &str, error: Option<&str>) -> String {
    let err_html = if let Some(msg) = error {
        format!("<div class='status-banner status-error'>{}</div>", msg)
    } else {
        String::new()
    };
    format!(r###"
        <div class="cyber-card" style="max-width: 400px; margin: 2rem auto;">
            <div class="card-title">Password Required</div>
            {}
            <form hx-post="/interaction/join" hx-target="#main-container">
                <input type="hidden" name="room_id" value="{}">
                <input type="hidden" name="role" value="{}">
                <input type="hidden" name="username" value="{}">

                <label class="field-label">Password</label>
                <input type="password" name="password" class="cyber-input" autofocus required>

                <div style="display: flex; gap: 0.6rem; margin-top: 1.25rem;">
                    <button type="submit" class="btn-cyber btn-g">[ Unlock ]</button>
                    <button hx-get="/interaction" hx-target="body" class="btn-cyber btn-p">[ Cancel ]</button>
                </div>
            </form>
        </div>
    "###, err_html, room_id, role, username)
}

pub async fn create_room(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateRoomForm>,
) -> impl IntoResponse {
    if form.username.trim().is_empty() {
        return Html("<div class='status-banner status-error'>Username cannot be empty. <button hx-get='/interaction' hx-target='body' class='btn-cyber btn-p btn-sm'>Back</button></div>".to_string()).into_response();
    }
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
    
    let password = if let Some(p) = form.password {
        if p.trim().is_empty() { None } else { Some(p) }
    } else {
        None
    };
    let room = Room {
        id: room_id.clone(),
        label: label.to_string(),
        tx,
        created_at: Utc::now(),
        users: HashMap::new(),
        max_controllers: form.max_controllers,
        max_doers: form.max_doers,
        current_color: default_color.clone(),
        password,
        uploaded_files: Vec::new(),
    };
    state.interaction.rooms.write().await.insert(room_id.clone(), room);
    Html(render_room_view(&room_id, user_role, &form.username, &default_color)).into_response()
}

pub async fn join_room(
    State(state): State<Arc<AppState>>,
    Form(form): Form<JoinRoomForm>,
) -> impl IntoResponse {
    if form.username.trim().is_empty() {
        return Html("<div class='status-banner status-error'>Username cannot be empty. <button hx-get='/interaction' hx-target='body' class='btn-cyber btn-p btn-sm'>Back</button></div>".to_string()).into_response();
    }
    let rooms = state.interaction.rooms.read().await;
    if let Some(room) = rooms.get(&form.room_id) {
        if let Some(required_pass) = &room.password {
            if form.password.as_deref() != Some(required_pass) {
                let err_msg = if form.password.is_some() { Some("Incorrect password") } else { None };
                return Html(render_password_prompt(&form.room_id, &form.role, &form.username, err_msg)).into_response();
            }
        }
        let role = match form.role.as_str() {
            "controller" => Role::Controller,
            _ => Role::Doer,
        };
        let count = room.users.values().filter(|u| u.role == role).count();
        let max = if role == Role::Controller { room.max_controllers } else { room.max_doers };
        if count >= max {
            return Html(format!("<div class='status-banner status-error'>Room is full for {:?}. <button hx-get='/interaction' hx-target='body' class='btn-cyber btn-p btn-sm'>Back</button></div>", role)).into_response();
        }
        Html(render_room_view(&room.id, role, &form.username, &room.current_color)).into_response()
    } else {
        Html("<div class='status-banner status-error'>Room not found. <button hx-get='/interaction' hx-target='body' class='btn-cyber btn-p btn-sm'>Back</button></div>".to_string()).into_response()
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
        _ => return Html("<div class='empty-state'>Invalid role selected</div>".to_string()),
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
        return Html("<div class='empty-state'>No rooms available.</div>".to_string());
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
        let lock_icon = if room.password.is_some() { "<span class='lock'>🔒</span> " } else { "" };
        html.push_str(&format!(r###"
            <div class="room-row">
                <div class="room-info">
                    <div class="room-name">{}{}</div>
                    <div class="room-meta">{} &bull; {}</div>
                </div>
                <form class="room-form" hx-post="/interaction/join" hx-target="#main-container">
                    <input type="hidden" name="username" value="{}">
                    <input type="hidden" name="role" value="{}">
                    <input type="hidden" name="room_id" value="{}">
                    <button type="submit" class="btn-cyber btn-g btn-sm">Join</button>
                </form>
            </div>
        "###,
        lock_icon,
        room.label,
        info,
        room.created_at.format("%H:%M:%S"),
        params.username,
        role_val,
        room.id,
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
    let mut saved: Option<(String, String)> = None; // (stored filename, content type)

    loop {
        match multipart.next_field().await {
            Ok(Some(mut field)) => {
                let name = field.name().unwrap_or_default().to_string();
                if name == "username" {
                    if let Ok(text) = field.text().await { username = text; }
                } else if name == "file" {
                    let orig_name = field.file_name().unwrap_or_default().to_string();
                    if orig_name.is_empty() { continue; }
                    let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();

                    // Prefix with a UUID so concurrent uploads from different users never collide.
                    let ext = std::path::Path::new(&orig_name).extension().and_then(|e| e.to_str());
                    let unique_name = match ext {
                        Some(ext) => format!("chat-{}.{}", Uuid::new_v4(), ext),
                        None => format!("chat-{}", Uuid::new_v4()),
                    };
                    let stored_name = sanitize_filename::sanitize(&unique_name);
                    let filepath = std::path::Path::new("./uploads").join(&stored_name);

                    if tokio::fs::create_dir_all("./uploads").await.is_err() {
                        return Html("<span style='color: var(--p);'>Upload Error</span>".to_string());
                    }
                    let mut file = match tokio::fs::File::create(&filepath).await {
                        Ok(f) => f,
                        Err(_) => return Html("<span style='color: var(--p);'>Upload Error</span>".to_string()),
                    };

                    // Snapshot storage usage once, then track this file's own size as we stream
                    // it to disk — avoids re-walking the whole uploads dir on every chunk.
                    let limit = state.config.upload_storage_limit;
                    let base_size = if limit.is_some() {
                        crate::upload::get_directory_size("./uploads").await
                    } else {
                        0
                    };
                    let mut written: u64 = 0;
                    let mut failed = false;
                    loop {
                        match field.chunk().await {
                            Ok(Some(chunk)) => {
                                written += chunk.len() as u64;
                                if let Some(max) = limit {
                                    if base_size + written > max {
                                        failed = true;
                                        break;
                                    }
                                }
                                if tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await.is_err() {
                                    failed = true;
                                    break;
                                }
                            },
                            Ok(None) => break,
                            Err(_) => { failed = true; break; },
                        }
                    }
                    drop(file);
                    if failed || written == 0 {
                        let _ = tokio::fs::remove_file(&filepath).await;
                        if failed && written > 0 {
                            return Html("<span style='color: var(--p);'>Upload limit exceeded.</span>".to_string());
                        }
                        return Html("<span style='color: var(--p);'>No file received.</span>".to_string());
                    }
                    saved = Some((stored_name, content_type));
                }
            },
            Ok(None) => break,
            Err(_) => return Html("<span style='color: var(--p);'>Upload Error</span>".to_string()),
        }
    }
    let (stored_name, content_type) = match saved {
        Some(v) => v,
        None => return Html("<span style='color: var(--p);'>No file received.</span>".to_string()),
    };
    let url = format!("/uploads/{}", stored_name);
    let content_html = if content_type.starts_with("image/") {
        format!(r###"<br><img src="{}" alt="{}">"###, url, stored_name)
    } else {
        format!(r###"Shared file: <a href="{}" download="{}">{}</a>"###, url, stored_name, stored_name)
    };
    let mut rooms = state.interaction.rooms.write().await;
    if let Some(room) = rooms.get_mut(&room_id) {
         room.uploaded_files.push(stored_name.clone());
         let msg = format!(r###"<div hx-swap-oob="beforeend:#chat-container">
                <div class="chat-msg">
                    <span class="sender">{}: </span>
                    <span>{}</span>
                </div>
               </div>"###, username, content_html);
         let _ = room.tx.send(msg);
    } else {
        // Room vanished while this upload was in flight — nothing will ever own
        // this file's cleanup, so delete it now instead of leaking it on disk.
        drop(rooms);
        let _ = tokio::fs::remove_file(std::path::Path::new("./uploads").join(&stored_name)).await;
    }
    Html("<span style='color: var(--g);'>Uploaded!</span>".to_string())
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
    let connection_id = tokio::task::spawn_blocking(Uuid::new_v4)
        .await
        .unwrap()
        .to_string();
    let connection_id_inner = connection_id.clone();
    
    // FIX 4: Send the generated UUID back to the specific client so they know their identity
    let welcome_msg = serde_json::json!({
        "webrtc_welcome": connection_id_inner
    }).to_string();
    let _ = sender.send(Message::Text(welcome_msg.into())).await;

    let mut rx;
    {
        let mut rooms = state.interaction.rooms.write().await;
        if let Some(room) = rooms.get_mut(&room_id) {
            room.users.insert(connection_id.clone(), UserSession {
                role: role.clone(),
                username: username.clone(),
            });
            rx = room.tx.subscribe();
            let _ = room.tx.send(format!(
                r###"<div hx-swap-oob="beforeend:#chat-container"><div class="chat-system">{} ({}) joined.</div></div>"###,
                username, role
            ));
        } else {
            return;
        }
    }
    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if sender.send(Message::Text(msg.into())).await.is_err() { break; }
                },
                Err(_) => continue,
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
                    if !payload.webrtc_signal.is_empty() {
                        let broadcast_payload = serde_json::json!({
                            "webrtc": payload.webrtc_signal,
                            "sender_id": connection_id_inner,
                        });
                        let rooms = tx_inner_state.interaction.rooms.read().await;
                        if let Some(room) = rooms.get(&room_id_inner) {
                            let _ = room.tx.send(broadcast_payload.to_string());
                        }
                    }
                    if !payload.chat_message.is_empty() {
                        let msg_text = payload.chat_message.trim();
                        if msg_text.starts_with('/') {
                            let parts: Vec<&str> = msg_text.split_whitespace().collect();
                            if let Some(cmd_name) = parts.get(0) {
                                let command_name = &cmd_name[1..];
                                let args: Vec<String> = parts.iter().skip(1).map(|s| s.to_string()).collect();
                                
                                let room_tx = {
                                    let rooms = tx_inner_state.interaction.rooms.read().await;
                                    rooms.get(&room_id_inner).map(|r| r.tx.clone())
                                };
                                if let (Some(handler), Some(tx)) = (tx_inner_state.interaction.commands.get(command_name), room_tx) {
                                    handler(args, tx).await;
                                }
                            }
                        } else {
                            let safe_msg = msg_text.replace("<", "&lt;").replace(">", "&gt;");
                            let mut formatted_msg = safe_msg.clone();
                            if safe_msg.starts_with("http") {
                                formatted_msg = format!(r###"<a href="{}" target="_blank">{}</a>"###, safe_msg, safe_msg);
                            }

                            let html = format!(
                                r###"<div hx-swap-oob="beforeend:#chat-container">
                                    <div class="chat-msg">
                                        <span class="sender">{}: </span>
                                        <span>{}</span>
                                    </div>
                                   </div>"###,
                                username_inner, formatted_msg
                            );
                            
                            let rooms = tx_inner_state.interaction.rooms.read().await;
                            if let Some(room) = rooms.get(&room_id_inner) {
                                let _ = room.tx.send(html);
                            }
                        }
                    }
                    if !payload.signal.is_empty() {
                         let mut rooms = tx_inner_state.interaction.rooms.write().await;
                         if let Some(room) = rooms.get_mut(&room_id_inner) {
                            room.current_color = payload.signal.clone();
                            
                            let doer_html = format!(
                                r###"<div id="signal-circle" class="signal-circle" style="background-color: {};" hx-swap-oob="true"></div>"###,
                                payload.signal
                            );
                            let _ = room.tx.send(doer_html);

                            let btn_class = "signal-btn";
                            let active = |c: &str| if payload.signal == c { "active" } else { "" };
                            let controller_html = format!(r###"
                                <div id="view-controller" class="signal-panel" hx-swap-oob="true">
                                    <form hx-ws="send" class="signal-btn-row">
                                        <button class="{btn_class} sig-red {a_red}" name="signal" value="#ff3d6b">Red</button>
                                        <button class="{btn_class} sig-green {a_green}" name="signal" value="#00ff41">Green</button>
                                        <button class="{btn_class} sig-blue {a_blue}" name="signal" value="#00e5ff">Blue</button>
                                        <button class="{btn_class} sig-yellow {a_yellow}" name="signal" value="#ffb347">Yellow</button>
                                    </form>
                                </div>
                            "###,
                            btn_class=btn_class,
                            a_red=active("#ff3d6b"),
                            a_green=active("#00ff41"),
                            a_blue=active("#00e5ff"),
                            a_yellow=active("#ffb347")
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
            r###"<div hx-swap-oob="beforeend:#chat-container"><div class="chat-system">{} left.</div></div>"###,
            username
        );
        let _ = room.tx.send(leave_msg);
        if room.users.is_empty() {
             if room.id.len() == 36 {
                if let Some(removed) = rooms.remove(&room_id) {
                    drop(rooms);
                    for filename in removed.uploaded_files {
                        let _ = tokio::fs::remove_file(std::path::Path::new("./uploads").join(&filename)).await;
                    }
                }
             }
        }
    }
}
