// Realtime rooms at /interaction: chat, controller → doer colour signals,
// file sharing and opt-in voice (WebRTC is peer to peer; the server only
// relays the signalling). Rooms live in memory.
//
// Joining goes through /interaction/create or /interaction/join, which check
// the password, capacity and name, then hand out a member token. The
// WebSocket, uploads and shared files all require that token, so they can't be
// used to skip those checks or to post under someone else's name.
//
// Everything users type ends up in HTML fragments pushed to other people's
// browsers, so every piece of user text goes through `esc`.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Form, Multipart, Path, Query, Request, State,
    },
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, RwLock};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use uuid::Uuid;

use crate::lab::{ip_key, RateLimiter};
use crate::AppState;

/// Messages kept per room and replayed to people who join later.
const HISTORY_LEN: usize = 100;
const MAX_MESSAGE_CHARS: usize = 2000;
const MAX_USERNAME_CHARS: usize = 32;
const MAX_ROOM_NAME_CHARS: usize = 40;
const MAX_PASSWORD_CHARS: usize = 128;
/// WebRTC offers/answers with candidates are a few KB.
const MAX_WS_MESSAGE: usize = 64 * 1024;
const MAX_ROOMS: usize = 200;
const ROOMS_PER_IP_PER_HOUR: usize = 10;
/// Chat messages per connection per 10 seconds.
const CHAT_BURST: usize = 8;
const UPLOADS_PER_MINUTE: usize = 6;
/// A member token with no open connection is forgotten after this. Long
/// enough to ride out a dropped connection; htmx reconnects on its own.
const TOKEN_IDLE: Duration = Duration::from_secs(120);
/// Chat uploads live outside every publicly served directory and are only
/// reachable through `serve_file`, which checks the room still exists.
const UPLOAD_DIR: &str = "./chat-uploads";
const IMAGE_EXTS: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
];
/// Controller signals: (form value, colour, label).
const SIGNALS: &[(&str, &str, &str)] = &[
    ("red", "#ff3d6b", "Red"),
    ("green", "#00ff41", "Green"),
    ("blue", "#00e5ff", "Blue"),
    ("yellow", "#ffb347", "Yellow"),
];
const NO_SIGNAL: &str = "#808080";

static CREATE_LIMIT: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(ROOMS_PER_IP_PER_HOUR, Duration::from_secs(3600)));

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Role {
    Controller,
    Doer,
}

impl Role {
    fn parse(s: &str) -> Role {
        if s.eq_ignore_ascii_case("controller") { Role::Controller } else { Role::Doer }
    }
    fn value(self) -> &'static str {
        match self {
            Role::Controller => "controller",
            Role::Doer => "doer",
        }
    }
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
    #[serde(default)]
    pub name: String,
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
    #[serde(default)]
    role: String,
}

#[derive(Debug, Deserialize)]
pub struct LobbyParams {
    room: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WsParams {
    token: String,
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

/// Someone who has been let into a room. They may have several connections
/// (tabs) open, or none for a moment while reconnecting.
struct Member {
    username: String,
    role: Role,
    connections: usize,
    last_seen: Instant,
    uploads: VecDeque<Instant>,
}

struct SharedFile {
    stored: String,
    name: String,
}

pub struct Room {
    pub id: String,
    pub label: String,
    tx: broadcast::Sender<String>,
    created_at: DateTime<Utc>,
    /// Keyed by member token.
    members: HashMap<String, Member>,
    max_controllers: usize,
    max_doers: usize,
    signal: Option<&'static str>,
    password: Option<String>,
    files: Vec<SharedFile>,
    /// Rendered chat lines (without the oob wrapper), oldest first.
    history: VecDeque<String>,
    /// Rooms from config.toml stay when empty; they only forget their history.
    permanent: bool,
}

impl Room {
    pub fn new(id: String, label: String, max_controllers: usize, max_doers: usize, password: Option<String>, permanent: bool) -> Self {
        let (tx, _rx) = broadcast::channel(256);
        Room {
            id,
            label,
            tx,
            created_at: Utc::now(),
            members: HashMap::new(),
            max_controllers,
            max_doers,
            signal: None,
            password,
            files: Vec::new(),
            history: VecDeque::new(),
            permanent,
        }
    }

    fn role_count(&self, role: Role) -> usize {
        self.members.values().filter(|m| m.role == role).count()
    }

    fn online_count(&self, role: Role) -> usize {
        self.members.values().filter(|m| m.role == role && m.connections > 0).count()
    }

    fn capacity(&self, role: Role) -> usize {
        match role {
            Role::Controller => self.max_controllers,
            Role::Doer => self.max_doers,
        }
    }

    fn name_taken(&self, username: &str) -> bool {
        self.members.values().any(|m| m.username.to_lowercase() == username.to_lowercase())
    }

    fn admit(&mut self, username: String, role: Role) -> String {
        let token = Uuid::new_v4().simple().to_string();
        self.members.insert(token.clone(), Member {
            username,
            role,
            connections: 0,
            last_seen: Instant::now(),
            uploads: VecDeque::new(),
        });
        token
    }

    /// Adds a line to the chat for everyone, and to the history.
    fn post(&mut self, line: String) {
        let _ = self.tx.send(oob_append(&line));
        self.history.push_back(line);
        while self.history.len() > HISTORY_LEN {
            self.history.pop_front();
        }
    }

    fn history_html(&self) -> String {
        let mut html = String::from(r#"<div id="chat-container" hx-swap-oob="innerHTML">"#);
        for line in &self.history {
            html.push_str(line);
        }
        html.push_str("</div>");
        html
    }

    fn user_list_html(&self) -> String {
        let mut online: Vec<&Member> = self.members.values().filter(|m| m.connections > 0).collect();
        online.sort_by(|a, b| (a.role.value(), a.username.to_lowercase()).cmp(&(b.role.value(), b.username.to_lowercase())));
        let mut html = String::from(r#"<ul id="user-list" class="user-list" hx-swap-oob="true">"#);
        for m in online {
            html.push_str(&format!(
                r#"<li><span class="user-dot"></span><span class="user-name">{}</span><span class="user-role">{}</span></li>"#,
                esc(&m.username), m.role
            ));
        }
        html.push_str("</ul>");
        html
    }

    fn signal_html(&self) -> String {
        format!("{}{}", controller_panel(self.signal, true), doer_circle(self.signal, true))
    }
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

// ─── HTML helpers ────────────────────────────────────────────────────────────

/// Escapes text for HTML element content and quoted attribute values.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Wraps a chat line so htmx appends it to the chat.
pub fn oob_append(line: &str) -> String {
    format!(r#"<div hx-swap-oob="beforeend:#chat-container">{}</div>"#, line)
}

/// A timestamp the page rewrites into the reader's local time.
fn time_tag() -> String {
    let now = Utc::now();
    format!(r#"<time data-t="{}">{}</time>"#, now.timestamp_millis(), now.format("%H:%M"))
}

/// Escapes a message and turns http(s) URLs in it into links.
fn linkify(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            if !(word.starts_with("https://") || word.starts_with("http://")) {
                return esc(word);
            }
            // Leave trailing punctuation out of the link: "see https://x.com)."
            let url = word.trim_end_matches(['.', ',', ')', '!', '?', ';', ':', '\'', '"']);
            if url.len() <= "https://".len() {
                return esc(word);
            }
            format!(
                r#"<a href="{0}" target="_blank" rel="noopener noreferrer nofollow">{0}</a>{1}"#,
                esc(url), esc(&word[url.len()..])
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn chat_line(username: &str, body_html: &str) -> String {
    format!(
        r#"<div class="chat-msg">{} <span class="sender">{}</span> <span class="text">{}</span></div>"#,
        time_tag(), esc(username), body_html
    )
}

pub fn system_line(text: &str) -> String {
    format!(r#"<div class="chat-system">{} {}</div>"#, time_tag(), esc(text))
}

fn controller_panel(current: Option<&str>, oob: bool) -> String {
    let mut buttons = String::new();
    for (value, _, label) in SIGNALS {
        let active = current == Some(*value);
        buttons.push_str(&format!(
            r#"<button type="button" class="signal-btn sig-{value}{cls}" ws-send name="signal" value="{value}" aria-pressed="{active}" title="{label}">{label}</button>"#,
            cls = if active { " active" } else { "" },
        ));
    }
    format!(
        r#"<div id="view-controller" class="signal-panel"{}><div class="signal-btn-row">{}</div><p class="signal-watch-label">Pick the colour everyone watching sees</p></div>"#,
        if oob { r#" hx-swap-oob="true""# } else { "" },
        buttons
    )
}

fn doer_circle(current: Option<&str>, oob: bool) -> String {
    let colour = current
        .and_then(|c| SIGNALS.iter().find(|(v, _, _)| *v == c))
        .map(|(_, hex, _)| *hex)
        .unwrap_or(NO_SIGNAL);
    format!(
        r#"<div id="signal-circle" class="signal-circle" style="background-color: {};"{}></div>"#,
        colour,
        if oob { r#" hx-swap-oob="true""# } else { "" }
    )
}

/// An error shown on the lobby (or password form) without replacing the page.
fn lobby_error(msg: &str) -> Response {
    let mut resp = Html(format!(r#"<div class="status-banner status-error">{}</div>"#, esc(msg))).into_response();
    let h = resp.headers_mut();
    h.insert("HX-Retarget", HeaderValue::from_static("#lobby-error"));
    h.insert("HX-Reswap", HeaderValue::from_static("innerHTML"));
    resp
}

fn clean_username(raw: &str) -> Result<String, &'static str> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("Pick a name first.");
    }
    if name.chars().count() > MAX_USERNAME_CHARS {
        return Err("Names can be up to 32 characters.");
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("That name has characters that aren't allowed.");
    }
    Ok(name)
}

fn render_room_view(room: &Room, role: Role, username: &str, token: &str) -> String {
    let panel = match role {
        Role::Controller => controller_panel(room.signal, false),
        Role::Doer => format!(
            r#"<div id="view-doer" class="signal-panel">{}<p class="signal-watch-label">Watch the circle</p></div>"#,
            doer_circle(room.signal, false)
        ),
    };
    format!(
        r##"<div id="room-container" class="room" hx-ext="ws" ws-connect="/ws/interaction/{rid_url}?token={token}" data-room="{rid}">
    <div class="room-view-header">
        <h2 class="room-view-title">{label}{lock} <span class="role-tag">// {role} · {user}</span></h2>
        <div class="room-actions">
            <button type="button" class="btn-cyber btn-c btn-sm" id="invite-btn">Copy invite link</button>
            <a href="/interaction" class="btn-cyber btn-p btn-sm">Leave</a>
        </div>
    </div>
    <div class="room-layout">
        <div class="room-main">
            {panel}
            <div class="chat-label">Chat <span id="conn-status" class="conn-status">connecting…</span></div>
            <div id="chat-container" class="chat-container" role="log" aria-live="polite"></div>
            <div class="chat-input-row">
                <form id="chat-form" ws-send class="chat-form">
                    <input type="text" name="chat_message" class="cyber-input chat-text-input" placeholder="Message, or /help" maxlength="{max_msg}" required autocomplete="off" aria-label="Message">
                    <button type="submit" class="btn-cyber btn-c">Send</button>
                </form>
                <form id="upload-form" hx-post="/interaction/upload/{rid_url}" hx-encoding="multipart/form-data" hx-target="#upload-status" class="upload-form">
                    <input type="hidden" name="token" value="{token}">
                    <label class="upload-label" title="Share a file">📎<input type="file" name="file" hidden></label>
                </form>
            </div>
            <div id="upload-status" class="upload-status" role="status"></div>
        </div>
        <aside class="room-side">
            <div class="side-title">Online</div>
            <ul id="user-list" class="user-list"></ul>
            <div class="side-title">Voice</div>
            <div class="voice-panel">
                <div class="voice-buttons">
                    <button type="button" id="voice-btn" class="btn-cyber btn-g btn-sm">Join voice</button>
                    <button type="button" id="mute-btn" class="btn-cyber btn-c btn-sm" hidden>Mute</button>
                </div>
                <span id="voice-status" class="voice-status">Off. Your mic is only used after you join.</span>
                <div id="remote-audio-container"></div>
            </div>
            <form id="webrtc-form" ws-send hidden><input type="hidden" id="webrtc-signal-input" name="webrtc_signal" value=""></form>
        </aside>
    </div>
</div>"##,
        rid_url = urlencoding::encode(&room.id),
        rid = esc(&room.id),
        token = token,
        label = esc(&room.label),
        lock = if room.password.is_some() { r#" <span class="lock" title="Password protected">🔒</span>"# } else { "" },
        role = role,
        user = esc(username),
        panel = panel,
        max_msg = MAX_MESSAGE_CHARS,
    )
}

fn render_password_prompt(room_id: &str, role: &str, username: &str, error: Option<&str>) -> String {
    let err_html = error
        .map(|msg| format!(r#"<div class="status-banner status-error">{}</div>"#, esc(msg)))
        .unwrap_or_default();
    format!(r##"
        <div class="cyber-card" style="max-width: 400px; margin: 2rem auto;">
            <div class="card-title">Password required</div>
            <div id="lobby-error" role="alert">{}</div>
            <form hx-post="/interaction/join" hx-target="#main-container">
                <input type="hidden" name="room_id" value="{}">
                <input type="hidden" name="role" value="{}">
                <input type="hidden" name="username" value="{}">
                <label class="field-label" for="room-password">Password</label>
                <input id="room-password" type="password" name="password" class="cyber-input" autofocus required>
                <div style="display: flex; gap: 0.6rem; margin-top: 1.25rem;">
                    <button type="submit" class="btn-cyber btn-g">[ Unlock ]</button>
                    <a href="/interaction" class="btn-cyber btn-p">[ Cancel ]</a>
                </div>
            </form>
        </div>
    "##, err_html, esc(room_id), esc(role), esc(username))
}

// ─── Lobby ───────────────────────────────────────────────────────────────────

pub async fn lobby_page(State(state): State<Arc<AppState>>, Query(params): Query<LobbyParams>) -> Response {
    let mut ctx = tera::Context::new();
    if let Some(id) = params.room {
        let rooms = state.interaction.rooms.read().await;
        match rooms.get(&id) {
            Some(room) => ctx.insert("invite", &serde_json::json!({
                "id": room.id,
                "label": room.label,
                "locked": room.password.is_some(),
            })),
            None => ctx.insert("invite_missing", &true),
        }
    }
    match state.tera.render("interaction.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            eprintln!("interaction render error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        }
    }
}

pub async fn create_room(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Form(form): Form<CreateRoomForm>,
) -> Response {
    let username = match clean_username(&form.username) {
        Ok(name) => name,
        Err(msg) => return lobby_error(msg),
    };
    let name = form.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.chars().count() > MAX_ROOM_NAME_CHARS {
        return lobby_error("Room names can be up to 40 characters.");
    }
    let password = form.password.filter(|p| !p.is_empty());
    if password.as_ref().is_some_and(|p| p.chars().count() > MAX_PASSWORD_CHARS) {
        return lobby_error("That password is too long.");
    }
    if !CREATE_LIMIT.allow(&ip_key(addr.ip())) {
        return lobby_error("You've made a lot of rooms recently. Try again in a bit.");
    }

    let mut rooms = state.interaction.rooms.write().await;
    let files = prune(&mut rooms);
    if rooms.values().filter(|r| !r.permanent).count() >= MAX_ROOMS {
        return lobby_error("There are too many rooms open right now. Try again later.");
    }
    let label = if name.is_empty() { format!("{}'s room", username) } else { name };
    let role = Role::parse(&form.role);
    let id = Uuid::new_v4().to_string();
    let mut room = Room::new(
        id.clone(),
        label,
        form.max_controllers.clamp(1, 50),
        form.max_doers.clamp(1, 100),
        password,
        false,
    );
    let token = room.admit(username.clone(), role);
    let html = render_room_view(&room, role, &username, &token);
    rooms.insert(id, room);
    drop(rooms);
    delete_files(files).await;
    Html(html).into_response()
}

pub async fn join_room(
    State(state): State<Arc<AppState>>,
    Form(form): Form<JoinRoomForm>,
) -> Response {
    let username = match clean_username(&form.username) {
        Ok(name) => name,
        Err(msg) => return lobby_error(msg),
    };
    let mut rooms = state.interaction.rooms.write().await;
    let files = prune(&mut rooms);
    let result = match rooms.get_mut(&form.room_id) {
        None => lobby_error("That room has closed."),
        Some(room) => {
            let role = Role::parse(&form.role);
            let password_ok = room.password.as_deref().is_none_or(|p| form.password.as_deref() == Some(p));
            if !password_ok {
                let err = form.password.is_some().then_some("Incorrect password");
                Html(render_password_prompt(&form.room_id, &form.role, &username, err)).into_response()
            } else if room.role_count(role) >= room.capacity(role) {
                lobby_error(&format!("That room has no free {} spots.", role.value()))
            } else if room.name_taken(&username) {
                lobby_error("Someone in that room is already using that name.")
            } else {
                let token = room.admit(username.clone(), role);
                Html(render_room_view(room, role, &username, &token)).into_response()
            }
        }
    };
    drop(rooms);
    delete_files(files).await;
    result
}

pub async fn list_rooms(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Html<String> {
    let role = Role::parse(&params.role);
    let rooms = state.interaction.rooms.read().await;
    let mut open: Vec<&Room> = rooms.values().filter(|r| r.role_count(role) < r.capacity(role)).collect();
    if open.is_empty() {
        return Html(r#"<div class="empty-state">No rooms with free spots. Create one.</div>"#.to_string());
    }
    // Permanent rooms first, then newest.
    open.sort_by(|a, b| b.permanent.cmp(&a.permanent).then(b.created_at.cmp(&a.created_at)));
    let mut html = String::from(r#"<div class="room-list">"#);
    for room in open {
        html.push_str(&format!(r##"
            <div class="room-row">
                <div class="room-info">
                    <div class="room-name">{lock}{label}</div>
                    <div class="room-meta">{c_on} of {c_max} controllers · {d_on} of {d_max} doers online</div>
                </div>
                <form class="room-form" hx-post="/interaction/join" hx-target="#main-container" hx-include="#join-username">
                    <input type="hidden" name="role" value="{role}">
                    <input type="hidden" name="room_id" value="{id}">
                    <button type="submit" class="btn-cyber btn-g btn-sm">Join</button>
                </form>
            </div>"##,
            lock = if room.password.is_some() { r#"<span class="lock" title="Password protected">🔒</span> "# } else { "" },
            label = esc(&room.label),
            c_on = room.online_count(Role::Controller),
            c_max = room.max_controllers,
            d_on = room.online_count(Role::Doer),
            d_max = room.max_doers,
            role = role.value(),
            id = esc(&room.id),
        ));
    }
    html.push_str("</div>");
    Html(html)
}

// ─── Files ───────────────────────────────────────────────────────────────────

fn upload_status(msg: &str, ok: bool) -> Response {
    Html(format!(r#"<span class="{}">{}</span>"#, if ok { "ok" } else { "err" }, esc(msg))).into_response()
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{} B", b),
        b if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

pub async fn upload_file(
    Path(room_id): Path<String>,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Response {
    // The token field comes first in the form, so it's checked before any of
    // the file is read.
    let mut token: Option<String> = None;
    loop {
        let mut field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => return upload_status("No file received.", false),
            Err(_) => return upload_status("Upload failed.", false),
        };
        match field.name() {
            Some("token") => token = field.text().await.ok(),
            Some("file") => {
                let Some(token) = token.as_deref() else {
                    return upload_status("You're not in this room.", false);
                };
                {
                    let mut rooms = state.interaction.rooms.write().await;
                    let Some(member) = rooms.get_mut(&room_id).and_then(|r| r.members.get_mut(token)) else {
                        return upload_status("You're not in this room.", false);
                    };
                    let now = Instant::now();
                    member.uploads.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
                    if member.uploads.len() >= UPLOADS_PER_MINUTE {
                        return upload_status("Slow down a little. Try again in a minute.", false);
                    }
                    member.uploads.push_back(now);
                }

                let orig = field.file_name().unwrap_or_default().to_string();
                let name: String = sanitize_filename::sanitize(&orig).chars().take(100).collect();
                if name.is_empty() {
                    return upload_status("No file received.", false);
                }
                let ext = std::path::Path::new(&name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .filter(|e| e.len() <= 10 && e.chars().all(|c| c.is_ascii_alphanumeric()));
                let stored = match &ext {
                    Some(ext) => format!("{}.{}", Uuid::new_v4().simple(), ext),
                    None => Uuid::new_v4().simple().to_string(),
                };
                let path = std::path::Path::new(UPLOAD_DIR).join(&stored);
                if tokio::fs::create_dir_all(UPLOAD_DIR).await.is_err() {
                    return upload_status("Upload failed.", false);
                }
                let Ok(mut file) = tokio::fs::File::create(&path).await else {
                    return upload_status("Upload failed.", false);
                };
                let limit = state.config.upload_storage_limit;
                let base_size = if limit.is_some() { crate::upload::get_directory_size(UPLOAD_DIR).await } else { 0 };
                let mut written: u64 = 0;
                let mut failed = false;
                loop {
                    match field.chunk().await {
                        Ok(Some(chunk)) => {
                            written += chunk.len() as u64;
                            if limit.is_some_and(|max| base_size + written > max) {
                                failed = true;
                                break;
                            }
                            if tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await.is_err() {
                                failed = true;
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => { failed = true; break; }
                    }
                }
                drop(file);
                if failed || written == 0 {
                    let _ = tokio::fs::remove_file(&path).await;
                    return upload_status(if written == 0 { "That file is empty." } else { "Upload failed or too large." }, false);
                }

                let url = format!("/interaction/file/{}/{}", urlencoding::encode(&room_id), stored);
                let is_image = ext.as_deref().is_some_and(|e| IMAGE_EXTS.iter().any(|(x, _)| *x == e));
                let body = if is_image {
                    format!(r#"<a href="{0}" target="_blank" rel="noopener"><img src="{0}" alt="{1}" loading="lazy"></a>"#, url, esc(&name))
                } else {
                    format!(r#"shared <a href="{}" download="{}">{}</a> <span class="file-size">({})</span>"#,
                        url, esc(&name), esc(&name), human_size(written))
                };
                let mut rooms = state.interaction.rooms.write().await;
                let Some(room) = rooms.get_mut(&room_id) else {
                    // The room closed mid-upload, so nothing would ever clean this up.
                    drop(rooms);
                    let _ = tokio::fs::remove_file(&path).await;
                    return upload_status("The room closed.", false);
                };
                let Some(username) = room.members.get(token).map(|m| m.username.clone()) else {
                    drop(rooms);
                    let _ = tokio::fs::remove_file(&path).await;
                    return upload_status("You're not in this room.", false);
                };
                room.files.push(SharedFile { stored, name });
                room.post(chat_line(&username, &body));
                return upload_status("Shared.", true);
            }
            _ => {}
        }
    }
}

/// Serves a file shared in a room, only while that room exists. Anything that
/// isn't a known image type is forced to download, and nothing is ever
/// rendered as a page on this origin.
pub async fn serve_file(
    State(state): State<Arc<AppState>>,
    Path((room_id, stored)): Path<(String, String)>,
    req: Request,
) -> Response {
    let name = {
        let rooms = state.interaction.rooms.read().await;
        rooms.get(&room_id)
            .and_then(|r| r.files.iter().find(|f| f.stored == stored))
            .map(|f| f.name.clone())
    };
    let Some(name) = name else {
        return (StatusCode::NOT_FOUND, "This file is gone. Shared files are deleted when their room closes.").into_response();
    };
    let path = std::path::Path::new(UPLOAD_DIR).join(&stored);
    let mut resp = match ServeFile::new(&path).oneshot(req).await {
        Ok(r) => r.into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Couldn't read the file").into_response(),
    };
    let ext = stored.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let h = resp.headers_mut();
    match IMAGE_EXTS.iter().find(|(x, _)| *x == ext) {
        Some((_, mime)) => {
            h.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
        }
        None => {
            h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
            if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{}", urlencoding::encode(&name))) {
                h.insert(header::CONTENT_DISPOSITION, v);
            }
        }
    }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; sandbox"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    h.insert("x-robots-tag", HeaderValue::from_static("noindex"));
    resp
}

// ─── Cleanup ─────────────────────────────────────────────────────────────────

/// Forgets idle member tokens and closes rooms nobody is in. Returns the shared
/// files to delete, so the caller can do that after releasing the lock.
fn prune(rooms: &mut HashMap<String, Room>) -> Vec<String> {
    let mut files = Vec::new();
    rooms.retain(|_, room| {
        room.members.retain(|_, m| m.connections > 0 || m.last_seen.elapsed() < TOKEN_IDLE);
        if !room.members.is_empty() {
            return true;
        }
        files.extend(room.files.drain(..).map(|f| f.stored));
        room.history.clear();
        room.signal = None;
        room.permanent
    });
    files
}

async fn delete_files(files: Vec<String>) {
    for stored in files {
        let _ = tokio::fs::remove_file(std::path::Path::new(UPLOAD_DIR).join(stored)).await;
    }
}

/// Clears shared files left over from before a restart (rooms don't survive
/// one) and prunes rooms in the background.
pub fn spawn_cleanup(state: Arc<AppState>) {
    tokio::spawn(async move {
        let _ = tokio::fs::remove_dir_all(UPLOAD_DIR).await;
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            let files = prune(&mut *state.interaction.rooms.write().await);
            delete_files(files).await;
        }
    });
}

// ─── WebSocket ───────────────────────────────────────────────────────────────

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    Query(params): Query<WsParams>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let known = state.interaction.rooms.read().await
        .get(&room_id)
        .is_some_and(|r| r.members.contains_key(&params.token));
    if !known {
        return (StatusCode::FORBIDDEN, "Join the room first").into_response();
    }
    ws.max_message_size(MAX_WS_MESSAGE)
        .max_frame_size(MAX_WS_MESSAGE)
        .on_upgrade(move |socket| handle_socket(socket, room_id, params.token, state))
}

const HELP: &str = "Commands: /me <action> describes what you're doing. /countdown [seconds] [message] counts down for everyone. /help shows this.";

async fn handle_socket(socket: WebSocket, room_id: String, token: String, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let connection_id = Uuid::new_v4().to_string();

    let (username, role, mut rx, catch_up) = {
        let mut rooms = state.interaction.rooms.write().await;
        let Some(room) = rooms.get_mut(&room_id) else { return };
        let Some(member) = room.members.get_mut(&token) else { return };
        member.connections += 1;
        member.last_seen = Instant::now();
        let first = member.connections == 1;
        let (username, role) = (member.username.clone(), member.role);
        // Subscribe and snapshot under the same lock so nothing falls between them.
        let rx = room.tx.subscribe();
        let catch_up = format!("{}{}{}", room.history_html(), room.signal_html(), room.user_list_html());
        if first {
            room.post(system_line(&format!("{} joined as {}.", username, role.value())));
            let _ = room.tx.send(room.user_list_html());
        }
        (username, role, rx, catch_up)
    };

    let welcome = serde_json::json!({ "webrtc_welcome": connection_id }).to_string();
    if sender.send(Message::Text(welcome.into())).await.is_err()
        || sender.send(Message::Text(catch_up.into())).await.is_err()
    {
        disconnect(&state, &room_id, &token).await;
        return;
    }

    // Replies meant only for this connection (errors, /help).
    let (direct_tx, mut direct_rx) = mpsc::channel::<String>(16);
    let mut send_task = tokio::spawn(async move {
        loop {
            let msg = tokio::select! {
                r = rx.recv() => match r {
                    Ok(msg) => msg,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                Some(msg) = direct_rx.recv() => msg,
            };
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let state_inner = state.clone();
    let room_inner = room_id.clone();
    let connection_inner = connection_id.clone();
    let mut recv_task = tokio::spawn(async move {
        let mut recent: VecDeque<Instant> = VecDeque::new();
        let reply = |text: &str| {
            let _ = direct_tx.try_send(oob_append(&system_line(text)));
        };
        while let Some(Ok(msg)) = receiver.next().await {
            let Message::Text(text) = msg else { continue };
            let Ok(payload) = serde_json::from_str::<HtmxWsMessage>(&text) else { continue };

            if !payload.webrtc_signal.is_empty() {
                let relay = serde_json::json!({
                    "webrtc": payload.webrtc_signal,
                    "sender_id": connection_inner,
                }).to_string();
                if let Some(room) = state_inner.interaction.rooms.read().await.get(&room_inner) {
                    let _ = room.tx.send(relay);
                }
            }

            if !payload.signal.is_empty() && role == Role::Controller {
                if let Some((value, _, _)) = SIGNALS.iter().find(|(v, _, _)| *v == payload.signal) {
                    let mut rooms = state_inner.interaction.rooms.write().await;
                    if let Some(room) = rooms.get_mut(&room_inner) {
                        room.signal = Some(value);
                        let _ = room.tx.send(room.signal_html());
                    }
                }
            }

            let msg = payload.chat_message.trim();
            if msg.is_empty() {
                continue;
            }
            let now = Instant::now();
            recent.retain(|t| now.duration_since(*t) < Duration::from_secs(10));
            if recent.len() >= CHAT_BURST {
                reply("You're sending messages too fast. Wait a few seconds.");
                continue;
            }
            recent.push_back(now);
            if msg.chars().count() > MAX_MESSAGE_CHARS {
                reply("Messages can be up to 2000 characters.");
                continue;
            }

            if let Some(command) = msg.strip_prefix('/') {
                let (name, rest) = command.split_once(' ').unwrap_or((command, ""));
                let rest = rest.trim();
                match name {
                    "help" => reply(HELP),
                    "me" if !rest.is_empty() => {
                        let line = format!(
                            r#"<div class="chat-msg chat-me">{} * <span class="sender">{}</span> {}</div>"#,
                            time_tag(), esc(&username), linkify(rest)
                        );
                        if let Some(room) = state_inner.interaction.rooms.write().await.get_mut(&room_inner) {
                            room.post(line);
                        }
                    }
                    _ => {
                        let room_tx = state_inner.interaction.rooms.read().await
                            .get(&room_inner)
                            .map(|r| r.tx.clone());
                        match (state_inner.interaction.commands.get(name), room_tx) {
                            (Some(handler), Some(tx)) => {
                                let args = rest.split_whitespace().map(str::to_string).collect();
                                // Commands can run for a while (/countdown); don't stall this socket.
                                tokio::spawn(handler(args, tx));
                            }
                            _ => reply(&format!("There's no /{} command. Try /help.", name)),
                        }
                    }
                }
                continue;
            }

            if let Some(room) = state_inner.interaction.rooms.write().await.get_mut(&room_inner) {
                room.post(chat_line(&username, &linkify(msg)));
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
    disconnect(&state, &room_id, &token).await;
}

async fn disconnect(state: &AppState, room_id: &str, token: &str) {
    let mut rooms = state.interaction.rooms.write().await;
    let Some(room) = rooms.get_mut(room_id) else { return };
    let Some(member) = room.members.get_mut(token) else { return };
    member.connections = member.connections.saturating_sub(1);
    member.last_seen = Instant::now();
    if member.connections == 0 {
        let line = system_line(&format!("{} left.", member.username));
        room.post(line);
        let _ = room.tx.send(room.user_list_html());
    }
}
