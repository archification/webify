// Shared live canvas for the Lab: a small pixel board everyone edits at once
// over WebSocket, with live cursors and an online count. No accounts and no
// text, so there's little to moderate; drawing is rate-limited per connection
// and connections are capped per IP. The board is saved to `lab_board.dat`
// once a minute when it has changed, and survives restarts.

use axum::{
    extract::{
        ConnectInfo, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

use crate::AppState;

pub const WIDTH: usize = 64;
pub const HEIGHT: usize = 40;
/// Palette size; index 0 is the empty background. Colors live in the page.
const COLORS: u8 = 12;
const SAVE_PATH: &str = "lab_board.dat";

const MAX_CONNECTIONS: usize = 300;
const MAX_CONNECTIONS_PER_IP: usize = 4;
/// Pixel token bucket: burst size and refill per second.
const PIXEL_BURST: f64 = 20.0;
const PIXELS_PER_SEC: f64 = 6.0;
/// Cursor updates beyond this rate are dropped.
const CURSOR_INTERVAL: Duration = Duration::from_millis(50);
const MAX_MESSAGE_BYTES: usize = 200;

struct Board {
    pixels: Mutex<Vec<u8>>,
    dirty: AtomicBool,
    tx: broadcast::Sender<String>,
    online: AtomicUsize,
    next_id: AtomicU64,
    per_ip: Mutex<HashMap<String, usize>>,
}

static BOARD: LazyLock<Board> = LazyLock::new(|| {
    let mut pixels = std::fs::read(SAVE_PATH).unwrap_or_default();
    if pixels.len() != WIDTH * HEIGHT || pixels.iter().any(|&c| c >= COLORS) {
        pixels = vec![0; WIDTH * HEIGHT];
    }
    tokio::spawn(async {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            if BOARD.dirty.swap(false, Ordering::Relaxed) {
                let snapshot = BOARD.pixels.lock().unwrap().clone();
                if let Err(e) = tokio::fs::write(SAVE_PATH, snapshot).await {
                    eprintln!("board: failed to save {}: {}", SAVE_PATH, e);
                }
            }
        }
    });
    Board {
        pixels: Mutex::new(pixels),
        dirty: AtomicBool::new(false),
        tx: broadcast::channel(1024).0,
        online: AtomicUsize::new(0),
        next_id: AtomicU64::new(1),
        per_ip: Mutex::new(HashMap::new()),
    }
});

#[derive(Deserialize)]
#[serde(tag = "t")]
enum ClientMsg {
    #[serde(rename = "px")]
    Pixel { x: usize, y: usize, c: u8 },
    #[serde(rename = "cur")]
    Cursor { x: f32, y: f32 },
}

/// Pixels encoded one base-36 digit per cell, row by row.
fn encode(pixels: &[u8]) -> String {
    pixels.iter().map(|&c| std::char::from_digit(c as u32, 36).unwrap_or('0')).collect()
}

pub async fn page(State(state): State<Arc<AppState>>, Extension(m): Extension<crate::lab::Mount>) -> Response {
    let mut ctx = tera::Context::new();
    ctx.insert("lab_path", m.path.as_str());
    ctx.insert("width", &WIDTH);
    ctx.insert("height", &HEIGHT);
    match state.tera.render("lab-board.html", &ctx) {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(e) => {
            eprintln!("board render error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        }
    }
}

pub async fn ws(ws: WebSocketUpgrade, ConnectInfo(addr): ConnectInfo<SocketAddr>) -> Response {
    let ip = crate::lab::ip_key(addr.ip());
    {
        let mut per_ip = BOARD.per_ip.lock().unwrap();
        let n = per_ip.entry(ip.clone()).or_insert(0);
        if *n >= MAX_CONNECTIONS_PER_IP || BOARD.online.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
            return (StatusCode::TOO_MANY_REQUESTS, "Too many open connections").into_response();
        }
        *n += 1;
        BOARD.online.fetch_add(1, Ordering::Relaxed);
    }
    ws.max_message_size(MAX_MESSAGE_BYTES * 4)
        .on_upgrade(move |socket| async move {
            session(socket).await;
            let mut per_ip = BOARD.per_ip.lock().unwrap();
            if let Some(n) = per_ip.get_mut(&ip) {
                *n -= 1;
                if *n == 0 {
                    per_ip.remove(&ip);
                }
            }
            let online = BOARD.online.fetch_sub(1, Ordering::Relaxed) - 1;
            let _ = BOARD.tx.send(json!({ "t": "n", "n": online }).to_string());
        })
}

async fn session(socket: WebSocket) {
    let id = BOARD.next_id.fetch_add(1, Ordering::Relaxed);
    let mut rx = BOARD.tx.subscribe();
    let (mut sink, mut stream) = socket.split();

    let init = json!({
        "t": "init",
        "id": id,
        "w": WIDTH,
        "h": HEIGHT,
        "px": encode(&BOARD.pixels.lock().unwrap()),
    });
    if sink.send(Message::Text(init.to_string().into())).await.is_err() {
        return;
    }
    let _ = BOARD.tx.send(json!({ "t": "n", "n": BOARD.online.load(Ordering::Relaxed) }).to_string());

    // Forward broadcasts to this client. A client too slow to keep up skips
    // ahead rather than holding the channel back.
    let mut forward = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if sink.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    let mut tokens = PIXEL_BURST;
    let mut last_refill = Instant::now();
    let mut last_cursor = Instant::now() - CURSOR_INTERVAL;
    loop {
        let msg = tokio::select! {
            m = stream.next() => m,
            _ = &mut forward => break,
        };
        let text = match msg {
            Some(Ok(Message::Text(t))) => t,
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
            Some(Ok(_)) => continue,
        };
        if text.len() > MAX_MESSAGE_BYTES {
            continue;
        }
        match serde_json::from_str::<ClientMsg>(&text) {
            Ok(ClientMsg::Pixel { x, y, c }) => {
                let now = Instant::now();
                tokens = (tokens + now.duration_since(last_refill).as_secs_f64() * PIXELS_PER_SEC).min(PIXEL_BURST);
                last_refill = now;
                if tokens < 1.0 || x >= WIDTH || y >= HEIGHT || c >= COLORS {
                    continue;
                }
                tokens -= 1.0;
                let changed = {
                    let mut px = BOARD.pixels.lock().unwrap();
                    let cell = &mut px[y * WIDTH + x];
                    let changed = *cell != c;
                    *cell = c;
                    changed
                };
                if changed {
                    BOARD.dirty.store(true, Ordering::Relaxed);
                    let _ = BOARD.tx.send(json!({ "t": "px", "x": x, "y": y, "c": c }).to_string());
                }
            }
            Ok(ClientMsg::Cursor { x, y }) => {
                if last_cursor.elapsed() < CURSOR_INTERVAL || !x.is_finite() || !y.is_finite() {
                    continue;
                }
                last_cursor = Instant::now();
                let x = x.clamp(0.0, WIDTH as f32);
                let y = y.clamp(0.0, HEIGHT as f32);
                let _ = BOARD.tx.send(json!({ "t": "cur", "id": id, "x": x, "y": y }).to_string());
            }
            Err(_) => {}
        }
    }
    forward.abort();
    let _ = BOARD.tx.send(json!({ "t": "bye", "id": id }).to_string());
}
