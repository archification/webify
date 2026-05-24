use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::RwLock;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::CookieJar;
use serde_json::json;
use uuid::Uuid;

use webrtc::{
    api::{
        APIBuilder,
        media_engine::MediaEngine,
        interceptor_registry::register_default_interceptors,
        setting_engine::SettingEngine,
    },
    ice_transport::{
        ice_server::RTCIceServer,
        ice_candidate::RTCIceCandidateInit,
        ice_candidate_type::RTCIceCandidateType,
    },
    interceptor::registry::Registry,
    peer_connection::{
        RTCPeerConnection,
        configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::RTPCodecType,
    track::track_local::{
        TrackLocal,
        TrackLocalWriter,
        track_local_static_rtp::TrackLocalStaticRTP,
    },
};

use crate::AppState;
use crate::forum::{ForumDb, Role};

pub struct StreamState {
    api: Arc<webrtc::api::API>,
    pub video_track: Arc<RwLock<Option<Arc<TrackLocalStaticRTP>>>>,
    pub audio_track: Arc<RwLock<Option<Arc<TrackLocalStaticRTP>>>>,
    pub is_live: Arc<AtomicBool>,
    broadcaster_pc: Arc<RwLock<Option<Arc<RTCPeerConnection>>>>,
    pub broadcaster_username: Arc<RwLock<Option<String>>>,
    viewer_pcs: Arc<RwLock<HashMap<String, Arc<RTCPeerConnection>>>>,
    whip_session: Arc<RwLock<Option<String>>>,
}

impl StreamState {
    pub fn new(public_ip: Option<String>) -> anyhow::Result<Self> {
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs()?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)?;

        let mut setting_engine = SettingEngine::default();
        if let Some(ip) = public_ip {
            setting_engine.set_nat_1to1_ips(vec![ip], RTCIceCandidateType::Host);
        }

        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_setting_engine(setting_engine)
            .build();

        Ok(Self {
            api: Arc::new(api),
            video_track: Arc::new(RwLock::new(None)),
            audio_track: Arc::new(RwLock::new(None)),
            is_live: Arc::new(AtomicBool::new(false)),
            broadcaster_pc: Arc::new(RwLock::new(None)),
            broadcaster_username: Arc::new(RwLock::new(None)),
            viewer_pcs: Arc::new(RwLock::new(HashMap::new())),
            whip_session: Arc::new(RwLock::new(None)),
        })
    }

    async fn new_peer_connection(&self) -> anyhow::Result<Arc<RTCPeerConnection>> {
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };
        Ok(Arc::new(self.api.new_peer_connection(config).await?))
    }
}

// ── DB helpers ────────────────────────────────────────────────────────────────

pub async fn get_or_create_stream_key(db: &ForumDb, username: &str) -> String {
    if let Ok(Some((key,))) = sqlx::query_as::<_, (String,)>(
        "SELECT stream_key FROM stream_keys WHERE username = ?",
    )
    .bind(username)
    .fetch_optional(&**db)
    .await
    {
        return key;
    }
    let key = Uuid::new_v4().to_string();
    let _ = sqlx::query("INSERT INTO stream_keys (username, stream_key) VALUES (?, ?)")
        .bind(username)
        .bind(&key)
        .execute(&**db)
        .await;
    key
}

pub async fn get_stream_key_owner(db: &ForumDb, key: &str) -> Option<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT username FROM stream_keys WHERE stream_key = ?",
    )
    .bind(key)
    .fetch_optional(&**db)
    .await
    .ok()
    .flatten()
    .map(|(u,)| u)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_owned)
}

fn parse_sdp_frag_candidates(body: &str) -> Vec<RTCIceCandidateInit> {
    let mut candidates = Vec::new();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("a=candidate:") {
            candidates.push(RTCIceCandidateInit {
                candidate: format!("candidate:{rest}"),
                sdp_mid: Some("0".to_owned()),
                sdp_mline_index: Some(0),
                username_fragment: None,
            });
        }
    }
    if body.contains("a=end-of-candidates") {
        candidates.push(RTCIceCandidateInit {
            candidate: String::new(),
            sdp_mid: Some("0".to_owned()),
            sdp_mline_index: Some(0),
            username_fragment: None,
        });
    }
    candidates
}

async fn wait_for_gather(pc: &Arc<RTCPeerConnection>) {
    let mut gather_complete = pc.gathering_complete_promise().await;
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        gather_complete.recv(),
    )
    .await;
}

// ── Route: GET /live ──────────────────────────────────────────────────────────

pub async fn live_handler(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> impl IntoResponse {
    let Some(user) = crate::forum::get_current_user(&state, &jar).await else {
        return Redirect::to("/forum/login").into_response();
    };

    if user.role < Role::Admin {
        return (
            StatusCode::FORBIDDEN,
            Html("<p style='font-family:sans-serif;padding:40px'>You don't have permission to stream.</p>"),
        )
            .into_response();
    }

    let stream_key = get_or_create_stream_key(&state.forum_db, &user.username).await;
    let is_live = state.stream.is_live.load(Ordering::Relaxed);
    let broadcaster = state
        .stream
        .broadcaster_username
        .read()
        .await
        .clone()
        .unwrap_or_default();

    let mut ctx = tera::Context::new();
    ctx.insert("username", &user.username);
    ctx.insert("stream_key", &stream_key);
    ctx.insert("is_live", &is_live);
    ctx.insert("broadcaster", &broadcaster);
    ctx.insert("domain", &state.config.domain);

    match state.tera.render("stream.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            eprintln!("Tera error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

// ── Route: GET /watch ─────────────────────────────────────────────────────────

pub async fn watch_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let is_live = state.stream.is_live.load(Ordering::Relaxed);
    let broadcaster = state
        .stream
        .broadcaster_username
        .read()
        .await
        .clone()
        .unwrap_or_default();

    let mut ctx = tera::Context::new();
    ctx.insert("is_live", &is_live);
    ctx.insert("broadcaster", &broadcaster);
    ctx.insert("domain", &state.config.domain);

    match state.tera.render("view.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            eprintln!("Tera error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

// ── Route: GET /watch/status ──────────────────────────────────────────────────

pub async fn watch_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let is_live = state.stream.is_live.load(Ordering::Relaxed);
    let broadcaster = state.stream.broadcaster_username.read().await.clone();
    axum::Json(json!({ "live": is_live, "broadcaster": broadcaster }))
}

// ── Route: POST /live/whip ────────────────────────────────────────────────────

pub async fn whip_ingest(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let Some(key) = extract_bearer(&headers) else {
        return (StatusCode::UNAUTHORIZED, "Missing Bearer token").into_response();
    };

    let Some(username) = get_stream_key_owner(&state.forum_db, &key).await else {
        return (StatusCode::UNAUTHORIZED, "Invalid stream key").into_response();
    };

    if state.stream.is_live.load(Ordering::Relaxed) {
        return (StatusCode::CONFLICT, "A stream is already active").into_response();
    }

    let pc = match state.stream.new_peer_connection().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("WHIP: create PC error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "PeerConnection error").into_response();
        }
    };

    let video_slot = state.stream.video_track.clone();
    let audio_slot = state.stream.audio_track.clone();
    let is_live_flag = state.stream.is_live.clone();
    let broadcaster_slot = state.stream.broadcaster_username.clone();
    let username_for_track = username.clone();

    pc.on_track(Box::new(move |remote_track, _, _| {
        let video_slot = video_slot.clone();
        let audio_slot = audio_slot.clone();
        let is_live_flag = is_live_flag.clone();
        let broadcaster_slot = broadcaster_slot.clone();
        let username = username_for_track.clone();

        Box::pin(async move {
            let kind = remote_track.kind();
            let capability = remote_track.codec().capability.clone();

            let local_track = Arc::new(TrackLocalStaticRTP::new(
                capability,
                Uuid::new_v4().to_string(),
                "broadcast".to_owned(),
            ));

            match kind {
                RTPCodecType::Video => {
                    *video_slot.write().await = Some(local_track.clone());
                }
                RTPCodecType::Audio => {
                    *audio_slot.write().await = Some(local_track.clone());
                }
                _ => return,
            }

            is_live_flag.store(true, Ordering::Relaxed);
            *broadcaster_slot.write().await = Some(username);

            // Forward RTP from broadcaster to all viewer TrackLocalStaticRTP
            let video_slot2 = video_slot.clone();
            let audio_slot2 = audio_slot.clone();
            let is_live2 = is_live_flag.clone();
            let broadcaster2 = broadcaster_slot.clone();

            tokio::spawn(async move {
                loop {
                    match remote_track.read_rtp().await {
                        Ok((packet, _)) => {
                            let _ = local_track.write_rtp(&packet).await;
                        }
                        Err(_) => {
                            // Track ended – clear state if this was the last track
                            match kind {
                                RTPCodecType::Video => {
                                    *video_slot2.write().await = None;
                                }
                                RTPCodecType::Audio => {
                                    *audio_slot2.write().await = None;
                                }
                                _ => {}
                            }
                            let video_gone = video_slot2.read().await.is_none();
                            let audio_gone = audio_slot2.read().await.is_none();
                            if video_gone && audio_gone {
                                is_live2.store(false, Ordering::Relaxed);
                                *broadcaster2.write().await = None;
                            }
                            break;
                        }
                    }
                }
            });
        })
    }));

    // Handle connection state transitions
    {
        let is_live_flag = state.stream.is_live.clone();
        let video_slot = state.stream.video_track.clone();
        let audio_slot = state.stream.audio_track.clone();
        let broadcaster_slot = state.stream.broadcaster_username.clone();
        let pc_slot = state.stream.broadcaster_pc.clone();
        let session_slot = state.stream.whip_session.clone();

        pc.on_peer_connection_state_change(Box::new(move |s| {
            let is_live_flag = is_live_flag.clone();
            let video_slot = video_slot.clone();
            let audio_slot = audio_slot.clone();
            let broadcaster_slot = broadcaster_slot.clone();
            let pc_slot = pc_slot.clone();
            let session_slot = session_slot.clone();

            Box::pin(async move {
                if matches!(
                    s,
                    RTCPeerConnectionState::Disconnected
                        | RTCPeerConnectionState::Failed
                        | RTCPeerConnectionState::Closed
                ) {
                    is_live_flag.store(false, Ordering::Relaxed);
                    *video_slot.write().await = None;
                    *audio_slot.write().await = None;
                    *broadcaster_slot.write().await = None;
                    *pc_slot.write().await = None;
                    *session_slot.write().await = None;
                }
            })
        }));
    }

    // SDP exchange
    let offer = match RTCSessionDescription::offer(body) {
        Ok(o) => o,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("Invalid SDP: {e}")).into_response(),
    };
    if let Err(e) = pc.set_remote_description(offer).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("SRD error: {e}")).into_response();
    }

    let answer = match pc.create_answer(None).await {
        Ok(a) => a,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Create answer: {e}"))
                .into_response()
        }
    };
    if let Err(e) = pc.set_local_description(answer).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("SLD error: {e}")).into_response();
    }

    wait_for_gather(&pc).await;

    let local_desc = match pc.local_description().await {
        Some(d) => d,
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "No local description").into_response()
        }
    };

    let session_id = Uuid::new_v4().to_string();
    *state.stream.whip_session.write().await = Some(session_id.clone());
    *state.stream.broadcaster_pc.write().await = Some(pc);
    *state.stream.broadcaster_username.write().await = Some(username);

    let location = format!("https://{}/live/whip/{}", state.config.domain, session_id);
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/sdp"),
    );
    resp_headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&location).unwrap(),
    );

    (StatusCode::CREATED, resp_headers, local_desc.sdp).into_response()
}

// ── Route: PATCH /live/whip/{id} ─────────────────────────────────────────────

pub async fn whip_patch(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    body: String,
) -> impl IntoResponse {
    if state.stream.whip_session.read().await.as_deref() != Some(&session_id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let pc = match state.stream.broadcaster_pc.read().await.clone() {
        Some(p) => p,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    for candidate in parse_sdp_frag_candidates(&body) {
        let _ = pc.add_ice_candidate(candidate).await;
    }
    StatusCode::NO_CONTENT.into_response()
}

// ── Route: DELETE /live/whip/{id} ────────────────────────────────────────────

pub async fn whip_delete(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    if state.stream.whip_session.read().await.as_deref() != Some(&session_id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some(pc) = state.stream.broadcaster_pc.write().await.take() {
        let _ = pc.close().await;
    }
    state.stream.is_live.store(false, Ordering::Relaxed);
    *state.stream.video_track.write().await = None;
    *state.stream.audio_track.write().await = None;
    *state.stream.broadcaster_username.write().await = None;
    *state.stream.whip_session.write().await = None;
    StatusCode::OK.into_response()
}

// ── Route: POST /watch/whep ───────────────────────────────────────────────────

pub async fn whep_handler(
    State(state): State<Arc<AppState>>,
    body: String,
) -> impl IntoResponse {
    if !state.stream.is_live.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "Stream is offline").into_response();
    }

    let video_track = state.stream.video_track.read().await.clone();
    let audio_track = state.stream.audio_track.read().await.clone();

    if video_track.is_none() && audio_track.is_none() {
        return (StatusCode::SERVICE_UNAVAILABLE, "No tracks yet").into_response();
    }

    let pc = match state.stream.new_peer_connection().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("WHEP: create PC error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "PeerConnection error").into_response();
        }
    };

    if let Some(v) = video_track {
        let track: Arc<dyn TrackLocal + Send + Sync> = v;
        if let Err(e) = pc.add_track(track).await {
            eprintln!("WHEP: add video track: {e}");
        }
    }
    if let Some(a) = audio_track {
        let track: Arc<dyn TrackLocal + Send + Sync> = a;
        if let Err(e) = pc.add_track(track).await {
            eprintln!("WHEP: add audio track: {e}");
        }
    }

    // Clean up this viewer's PC when the connection ends
    let session_id_cleanup = Uuid::new_v4().to_string();
    {
        let viewer_pcs_cleanup = state.stream.viewer_pcs.clone();
        let sid = session_id_cleanup.clone();
        pc.on_peer_connection_state_change(Box::new(move |s| {
            let viewer_pcs_cleanup = viewer_pcs_cleanup.clone();
            let sid = sid.clone();
            Box::pin(async move {
                if matches!(
                    s,
                    RTCPeerConnectionState::Disconnected
                        | RTCPeerConnectionState::Failed
                        | RTCPeerConnectionState::Closed
                ) {
                    viewer_pcs_cleanup.write().await.remove(&sid);
                }
            })
        }));
    }

    let offer = match RTCSessionDescription::offer(body) {
        Ok(o) => o,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("Invalid SDP: {e}")).into_response(),
    };
    if let Err(e) = pc.set_remote_description(offer).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("SRD error: {e}")).into_response();
    }

    let answer = match pc.create_answer(None).await {
        Ok(a) => a,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("Create answer: {e}"))
                .into_response()
        }
    };
    if let Err(e) = pc.set_local_description(answer).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("SLD error: {e}")).into_response();
    }

    wait_for_gather(&pc).await;

    let local_desc = match pc.local_description().await {
        Some(d) => d,
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "No local description").into_response()
        }
    };

    let session_id = session_id_cleanup;
    state
        .stream
        .viewer_pcs
        .write()
        .await
        .insert(session_id.clone(), pc);

    let location = format!("https://{}/watch/whep/{}", state.config.domain, session_id);
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/sdp"),
    );
    resp_headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&location).unwrap(),
    );

    (StatusCode::CREATED, resp_headers, local_desc.sdp).into_response()
}

// ── Route: PATCH /watch/whep/{id} ────────────────────────────────────────────

pub async fn whep_patch(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    body: String,
) -> impl IntoResponse {
    let pc = match state.stream.viewer_pcs.read().await.get(&session_id).cloned() {
        Some(p) => p,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    for candidate in parse_sdp_frag_candidates(&body) {
        let _ = pc.add_ice_candidate(candidate).await;
    }
    StatusCode::NO_CONTENT.into_response()
}
