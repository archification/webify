use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use surrealdb::sql::Thing;
use serde::{Deserialize, Serialize};
use axum::{
    extract::{State, Form, Query, Path},
    response::{Html, IntoResponse, Redirect},
    http::StatusCode,
};
use crate::AppState;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use bcrypt::{hash, verify, DEFAULT_COST};
use uuid::Uuid;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope,
    TokenResponse, TokenUrl, AuthorizationCode, HttpRequest, HttpResponse
};
use reqwest::Client as ReqwestClient;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub enum Role {
    Member = 0,
    Admin = 1,
    Owner = 2,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Member => write!(f, "Member"),
            Role::Admin => write!(f, "Admin"),
            Role::Owner => write!(f, "Owner"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Post {
    pub id: Option<Thing>,
    pub title: String,
    pub content: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub is_locked: bool,
    pub min_reply_role: Role,
    pub view_count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub id: Option<Thing>,
    pub thread_id: Thing,
    pub content: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    pub id: Option<Thing>,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub is_verified: bool,
    pub verification_token: String,
    pub role: Role,
}

#[derive(Deserialize)]
pub struct RegisterForm {
    pub username: String,
    pub email: String,
    pub password: String,
    pub confirm_password: String,
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize)]
pub struct VerifyQuery {
    pub token: String,
}

#[derive(Deserialize)]
pub struct GoogleCallback {
    pub code: String,
}

#[derive(Deserialize, Debug)]
pub struct GoogleUser {
    pub email: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct CreatePost {
    pub title: String,
    pub content: String,
    pub min_reply_role: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateReply {
    pub content: String,
}

pub type ForumDb = Arc<Surreal<Db>>;

pub async fn init_db() -> ForumDb {
    let db = Surreal::new::<surrealdb::engine::local::RocksDb>("forum.db").await.unwrap();
    db.use_ns("webify").use_db("forum").await.unwrap();
    Arc::new(db)
}

async fn get_current_user(state: &Arc<AppState>, jar: &CookieJar) -> Option<User> {
    if let Some(username_cookie) = jar.get("username") {
        let username = username_cookie.value().to_string();
        let mut result = state.forum_db.query("SELECT * FROM users WHERE username = $name")
            .bind(("name", username))
            .await.ok()?;
        let mut users: Vec<User> = result.take(0).ok()?;
        users.pop()
    } else {
        None
    }
}

pub async fn send_verification_email(
    config: &crate::config::Config,
    to_email: &str,
    token: &str,
) -> Result<(), String> {
    let smtp_server = config.smtp_server.as_deref().unwrap_or("localhost");
    let smtp_port = config.smtp_port.unwrap_or(25);
    let email = Message::builder()
        .from(config.email_from.as_deref().unwrap_or("no-reply@webify.local").parse().unwrap())
        .to(to_email.parse().unwrap())
        .subject("Verify your Webify account")
        .body(format!(
            "Please verify your account by clicking: http://{}/forum/verify?token={}",
            config.domain, token
        ))
        .map_err(|e| e.to_string())?;
    let mailer = if let (Some(user), Some(pass)) = (&config.smtp_username, &config.smtp_password) {
        let creds = Credentials::new(user.to_string(), pass.to_string());
        SmtpTransport::relay(smtp_server)
            .unwrap()
            .port(smtp_port)
            .credentials(creds)
            .build()
    } else {
        SmtpTransport::builder_dangerous(smtp_server)
            .port(smtp_port)
            .build()
    };
    mailer.send(&email).map_err(|e| e.to_string())?;
    Ok(())
}

// Handler: List all threads (Index)
pub async fn list_posts(
    State(state): State<Arc<AppState>>,
    jar: CookieJar
) -> impl IntoResponse {
    let mut result = state.forum_db.query("SELECT * FROM posts ORDER BY created_at DESC").await.unwrap();
    let posts: Vec<Post> = result.take(0).unwrap_or_default();
    
    let current_user = get_current_user(&state, &jar).await;
    
    let mut context = tera::Context::new();
    context.insert("posts", &posts);
    context.insert("current_user", &current_user);
    context.insert("base_path", &"/forum");
    
    match state.tera.render("forum.html", &context) {
        Ok(rendered) => Html(rendered).into_response(),
        Err(e) => {
            eprintln!("Tera error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

// Handler: View a specific thread and its replies
pub async fn view_thread(
    State(state): State<Arc<AppState>>,
    Path(thread_id_str): Path<String>,
    jar: CookieJar
) -> impl IntoResponse {
    let thread_id_formatted = if thread_id_str.contains(':') {
        thread_id_str.clone()
    } else {
        format!("posts:{}", thread_id_str)
    };

    // Fix: Clone thread_id_formatted for each bind call
    let thread_thing: Option<Thing> = state.forum_db.query("SELECT * FROM type::thing($id)")
        .bind(("id", thread_id_formatted.clone()))
        .await
        .ok()
        .and_then(|mut r| r.take::<Vec<Post>>(0).ok())
        .and_then(|mut v| v.pop().and_then(|p| p.id));
    
    if thread_thing.is_none() {
         return (StatusCode::NOT_FOUND, "Thread not found").into_response();
    }

    // Increment view count
    let _ = state.forum_db.query("UPDATE type::thing($id) SET view_count += 1")
        .bind(("id", thread_id_formatted.clone()))
        .await;

    // Fetch Thread
    let mut result = state.forum_db.query("SELECT * FROM type::thing($id)")
        .bind(("id", thread_id_formatted.clone()))
        .await.unwrap();
    let threads: Vec<Post> = result.take(0).unwrap_or_default();
    let thread = threads.into_iter().next();

    // Fetch Replies
    let mut result = state.forum_db.query("SELECT * FROM replies WHERE thread_id = type::thing($id) ORDER BY created_at ASC")
        .bind(("id", thread_id_formatted))
        .await.unwrap();
    let replies: Vec<Reply> = result.take(0).unwrap_or_default();

    let current_user = get_current_user(&state, &jar).await;
    let can_reply = if let Some(ref t) = thread {
        if let Some(ref u) = current_user {
             if t.is_locked {
                 u.role >= Role::Admin
             } else {
                 u.role >= t.min_reply_role
             }
        } else {
            false
        }
    } else {
        false
    };

    let can_moderate = if let Some(ref u) = current_user {
        u.role >= Role::Admin
    } else {
        false
    };

    let mut context = tera::Context::new();
    context.insert("thread", &thread);
    context.insert("replies", &replies);
    context.insert("current_user", &current_user);
    context.insert("can_reply", &can_reply);
    context.insert("can_moderate", &can_moderate);
    context.insert("base_path", &"/forum");

    match state.tera.render("topic.html", &context) {
        Ok(rendered) => Html(rendered).into_response(),
        Err(e) => {
             eprintln!("Tera error: {}", e);
             (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

// Handler: Post a reply
pub async fn post_reply(
    State(state): State<Arc<AppState>>,
    Path(thread_id_str): Path<String>,
    jar: CookieJar,
    Form(form): Form<CreateReply>,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to("/forum/login").into_response(),
    };

    let thread_id_formatted = if thread_id_str.contains(':') {
        thread_id_str.clone()
    } else {
        format!("posts:{}", thread_id_str)
    };

    // Fix: Clone thread_id_formatted here
    let mut result = state.forum_db.query("SELECT * FROM type::thing($id)")
        .bind(("id", thread_id_formatted.clone()))
        .await.unwrap();
    let threads: Vec<Post> = result.take(0).unwrap_or_default();
    let thread = threads.into_iter().next();

    if let Some(t) = thread {
        if t.is_locked && current_user.role < Role::Admin {
            return (StatusCode::FORBIDDEN, "Thread is locked").into_response();
        }
        if current_user.role < t.min_reply_role {
            return (StatusCode::FORBIDDEN, "Insufficient permissions").into_response();
        }
        
        let reply = Reply {
            id: None,
            thread_id: t.id.unwrap(),
            content: form.content,
            author: current_user.username,
            created_at: Utc::now(),
        };

        let _: Option<Reply> = state.forum_db.create("replies").content(reply).await.unwrap();
        
        // Update thread updated_at
        let _ = state.forum_db.query("UPDATE type::thing($id) SET updated_at = $now")
            .bind(("id", thread_id_formatted)) // Last use, can consume
            .bind(("now", Utc::now()))
            .await;
            
        return Redirect::to(&format!("/forum/thread/{}", thread_id_str)).into_response();
    }
    
    (StatusCode::NOT_FOUND, "Thread not found").into_response()
}

// Handler: Toggle Lock (Admin only)
pub async fn toggle_lock(
    State(state): State<Arc<AppState>>,
    Path(thread_id_str): Path<String>,
    jar: CookieJar,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Login required").into_response(),
    };

    if current_user.role < Role::Admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }

    let thread_id_formatted = if thread_id_str.contains(':') {
        thread_id_str.clone()
    } else {
        format!("posts:{}", thread_id_str)
    };

    // We can use a query to flip the boolean
    let _ = state.forum_db.query("UPDATE type::thing($id) SET is_locked = <bool> !is_locked")
        .bind(("id", thread_id_formatted))
        .await;

    Redirect::to(&format!("/forum/thread/{}", thread_id_str)).into_response()
}

pub async fn register_form(State(_state): State<Arc<AppState>>) -> Html<String> {
    let base = "/forum";
    Html(format!(r#"
        <!DOCTYPE html>
        <html>
        <head><link rel="stylesheet" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></head>
        <body style="max-width: 400px; margin: 50px auto; padding: 20px; border: 1px solid #586e75;">
            <h1>Register</h1>
            <form action="{}/register" method="post">
                <input type="text" name="username" placeholder="Username" style="width:100%" required><br><br>
                <input type="email" name="email" placeholder="Email Address" style="width:100%" required><br><br>
                <input type="password" name="password" placeholder="Password" style="width:100%" required><br><br>
                <input type="password" name="confirm_password" placeholder="Confirm Password" style="width:100%" required><br><br>
                <button type="submit" style="width:100%">Create Account</button>
            </form>
            <p><a href="{}/login">Already have an account? Login</a></p>
        </body>
        </html>
    "#, base, base))
}

pub async fn register(
    State(state): State<Arc<AppState>>, 
    Form(form): Form<RegisterForm>
) -> impl IntoResponse {
    if form.password != form.confirm_password {
        return Html("<h1>Error</h1><p>Passwords do not match.</p>").into_response();
    }
    
    // Check if any users exist. If not, make this user an Owner.
    let result = state.forum_db.query("SELECT count() FROM users").await;
    let count: i64 = match result {
        Ok(mut r) => r.take::<Option<i64>>(0).unwrap_or(None).unwrap_or(0),
        Err(_) => 0,
    };
    
    let role = if count == 0 { Role::Owner } else { Role::Member };

    let token = Uuid::new_v4().to_string();
    let hashed = hash(form.password, DEFAULT_COST).unwrap();
    let user = User { 
        id: None,
        username: form.username.clone(), 
        email: form.email.clone(), 
        password_hash: hashed,
        is_verified: false,
        verification_token: token.clone(),
        role,
    };
    
    // Check if user exists
    let existing: Option<User> = state.forum_db.select(("users", &form.username)).await.ok().flatten();
    if existing.is_some() {
        return Html("<h1>Error</h1><p>Username taken.</p>").into_response();
    }

    let _: Option<User> = state.forum_db.create(("users", &form.username)).content(user).await.unwrap();
    let _ = send_verification_email(&state.config, &form.email, &token).await;
    Html("<h1>Check your email</h1><p>A verification link has been sent to your email.</p>").into_response()
}

pub async fn verify_email(
    State(state): State<Arc<AppState>>,
    Query(query): Query<VerifyQuery>,
) -> impl IntoResponse {
    let mut result = state.forum_db
        .query("SELECT * FROM users WHERE verification_token = $v_token")
        .bind(("v_token", query.token)) 
        .await
        .expect("Verification query failed"); 
    let users: Vec<User> = result.take(0).unwrap_or_default();
    if let Some(user) = users.into_iter().next() {
        let _: Option<User> = state.forum_db.update(("users", &user.username))
            .merge(serde_json::json!({ "is_verified": true }))
            .await
            .expect("Failed to update user status");
        Html("<h1>Verified!</h1><p>Your account is now active. <a href='/forum/login'>Login here</a>.</p>").into_response()
    } else {
        Html("<h1>Invalid link</h1><p>This verification link is invalid or has expired.</p>").into_response()
    }
}

pub async fn login_form(State(_state): State<Arc<AppState>>) -> Html<String> {
    let base = "/forum";
    Html(format!(r#"
        <!DOCTYPE html>
        <html>
        <head><link rel="stylesheet" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></head>
        <body style="max-width: 400px; margin: 50px auto; padding: 20px; border: 1px solid #586e75;">
            <h1>Login</h1>
            <form action="{}/login" method="post">
                <input type="text" name="username" placeholder="Username" style="width:100%" required><br><br>
                <input type="password" name="password" placeholder="Password" style="width:100%" required><br><br>
                <button type="submit" style="width:100%">Login</button>
            </form>
            <br>
            <div style="text-align: center;">
                <a href="{}/auth/google" style="background-color: #eee8d5; color: #073642; padding: 10px; text-decoration: none; display: block; border-radius: 4px;">Login with Google</a>
            </div>
            <br>
            <p><a href="{}/register">No account? Register</a></p>
        </body>
        </html>
    "#, base, base, base))
}

pub async fn login(
    State(state): State<Arc<AppState>>, 
    jar: CookieJar, 
    Form(form): Form<LoginForm>
) -> impl IntoResponse {
    let user: Option<User> = state.forum_db.select(("users", &form.username)).await.unwrap();
    if let Some(user) = user {
        if !user.is_verified {
            return Html("<h1>Please verify your email first</h1>").into_response();
        }
        if verify(form.password, &user.password_hash).unwrap() {
            let cookie = Cookie::build(("username", user.username)).path("/").http_only(true).build();
            return (jar.add(cookie), Redirect::to("/forum")).into_response();
        }
    }
    Html("<h1>Invalid Credentials</h1>").into_response()
}

pub async fn logout(
    State(_state): State<Arc<AppState>>,
    jar: CookieJar
) -> impl IntoResponse {
    let cookie = Cookie::build("username").path("/").build();
    (jar.remove(cookie), Redirect::to("/forum"))
}

pub async fn new_post_form(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    let base = "/forum";
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to(&format!("{}/login", base)).into_response(),
    };

    // Role check: Only Members and above can post
    if current_user.role < Role::Member {
        return Html("<h1>Insufficient permissions</h1>".to_string()).into_response();
    }

    let role_options = if current_user.role >= Role::Admin {
        r#"<br><label>Min Reply Role: <select name="min_reply_role">
            <option value="Member">Member</option>
            <option value="Admin">Admin</option>
            <option value="Owner">Owner</option>
           </select></label><br>"#
    } else {
        ""
    };

    Html(format!(r#"
        <!DOCTYPE html>
        <html>
        <head><link rel="stylesheet" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></head>
        <body style="max-width: 800px; margin: 0 auto; padding: 20px;">
            <h1>Create New Thread</h1>
            <form action="{}/create" method="post">
                <input type="text" name="title" placeholder="Title" required style="width: 100%; margin-bottom: 10px;"><br>
                <textarea name="content" placeholder="Content" rows="10" style="width:100%" required></textarea><br>
                {}
                <br>
                <button type="submit">Post Thread</button>
            </form>
        </body>
        </html>
    "#, base, role_options)).into_response()
}

pub async fn create_post(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(form): Form<CreatePost>,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to("/forum/login").into_response(),
    };

    if current_user.role < Role::Member {
         return (StatusCode::FORBIDDEN, "Insufficient permissions").into_response();
    }

    let min_reply_role = match form.min_reply_role.as_deref() {
        Some("Admin") => Role::Admin,
        Some("Owner") => Role::Owner,
        _ => Role::Member,
    };

    // Prevent non-admins from setting high restrictions
    let actual_min_role = if current_user.role >= Role::Admin {
        min_reply_role
    } else {
        Role::Member
    };

    let _: Option<Post> = state.forum_db.create("posts")
        .content(Post {
            id: None,
            title: form.title,
            content: form.content,
            author: current_user.username,
            created_at: Utc::now(),
            updated_at: None,
            is_locked: false,
            min_reply_role: actual_min_role,
            view_count: 0,
        })
        .await
        .unwrap();
    Redirect::to("/forum").into_response()
}

// ... OAuth implementation ...

#[derive(Debug)]
pub struct OAuthError(String);

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OAuth Error: {}", self.0)
    }
}

impl std::error::Error for OAuthError {}

pub async fn reqwest_async_http_client(
    request: HttpRequest,
) -> Result<HttpResponse, OAuthError> {
    let client = ReqwestClient::new();
    let (parts, body) = request.into_parts();
    let url = parts.uri.to_string(); 
    
    let mut builder = client.request(parts.method, &url).body(body);
    
    for (name, value) in parts.headers.iter() {
        builder = builder.header(name, value);
    }
    let response = builder.send().await.map_err(|e| OAuthError(e.to_string()))?;
    let mut builder = axum::http::Response::builder().status(response.status());
    for (name, value) in response.headers() {
        builder = builder.header(name, value);
    }
    let body = response.bytes().await.map_err(|e| OAuthError(e.to_string()))?.to_vec();
    Ok(builder.body(body).map_err(|e| OAuthError(e.to_string()))?)
}

pub async fn login_google(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let config = &state.config;
    if let (Some(id), Some(secret), Some(redirect)) = (
        &config.google_client_id,
        &config.google_client_secret,
        &config.google_redirect_url,
    ) {
        let client = BasicClient::new(ClientId::new(id.clone()))
            .set_client_secret(ClientSecret::new(secret.clone()))
            .set_auth_uri(AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string()).unwrap())
            .set_token_uri(TokenUrl::new("https://oauth2.googleapis.com/token".to_string()).unwrap())
            .set_redirect_uri(RedirectUrl::new(redirect.clone()).unwrap());
        let (auth_url, _csrf_token) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
            .add_extra_param("prompt", "select_account")
            .url();
        Redirect::to(auth_url.as_str()).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "Google OAuth not configured").into_response()
    }
}

pub async fn callback_google(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(query): Query<GoogleCallback>,
) -> impl IntoResponse {
    let config = &state.config;
    let (id, secret, redirect) = match (
        &config.google_client_id,
        &config.google_client_secret,
        &config.google_redirect_url,
    ) {
        (Some(i), Some(s), Some(r)) => (i, s, r),
        _ => return (StatusCode::INTERNAL_SERVER_ERROR, "Google OAuth not configured").into_response(),
    };
    let client = BasicClient::new(ClientId::new(id.clone()))
        .set_client_secret(ClientSecret::new(secret.clone()))
        .set_auth_uri(AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string()).unwrap())
        .set_token_uri(TokenUrl::new("https://oauth2.googleapis.com/token".to_string()).unwrap())
        .set_redirect_uri(RedirectUrl::new(redirect.clone()).unwrap());
    let token_result = client
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(&reqwest_async_http_client)
        .await;
    let token: oauth2::StandardTokenResponse<oauth2::EmptyExtraTokenFields, oauth2::basic::BasicTokenType> = match token_result {
        Ok(t) => t,
        Err(_) => return (StatusCode::BAD_REQUEST, "Failed to exchange token").into_response(),
    };
    let req_client = ReqwestClient::new();
    let user_info_resp = req_client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(token.access_token().secret())
        .send()
        .await;
    let user_info: GoogleUser = match user_info_resp {
        Ok(resp) => match resp.json().await {
            Ok(u) => u,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to parse Google user info").into_response(),
        },
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch Google user info").into_response(),
    };
    
    // Check if user exists
    let mut result = state.forum_db
        .query("SELECT * FROM users WHERE email = $email")
        .bind(("email", user_info.email.clone()))
        .await
        .unwrap();
    let users: Vec<User> = result.take(0).unwrap_or_default();
    
    let username = if let Some(user) = users.first() {
        user.username.clone()
    } else {
        // Create new user for Google Auth
        let base_username = user_info.name.replace(' ', "_");
        
        // Basic check for first user to be Owner
        let r_count = state.forum_db.query("SELECT count() FROM users").await;
        let count: i64 = match r_count { 
            Ok(mut r) => r.take::<Option<i64>>(0).unwrap_or(None).unwrap_or(0), 
            Err(_) => 0 
        };
        let role = if count == 0 { Role::Owner } else { Role::Member };

        let user = User {
            id: None,
            username: base_username.clone(),
            email: user_info.email.clone(),
            password_hash: "GOOGLE_OAUTH".to_string(),
            is_verified: true,
            verification_token: "google_oauth".to_string(),
            role,
        };
        match state.forum_db.create::<Option<User>>(("users", &base_username)).content(user.clone()).await {
            Ok(_) => base_username,
            Err(_) => {
                 let fallback_username = format!("{}_{}", base_username, Uuid::new_v4().to_string().split('-').next().unwrap());
                 let mut user_fallback = user.clone();
                 user_fallback.username = fallback_username.clone();
                 let _ : Option<User> = state.forum_db.create(("users", &fallback_username)).content(user_fallback).await.unwrap();
                 fallback_username
            }
        }
    };
    let cookie = Cookie::build(("username", username))
        .path("/")
        .http_only(true)
        .build();
    (jar.add(cookie), Redirect::to("/forum")).into_response()
}
