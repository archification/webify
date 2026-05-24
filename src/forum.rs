use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
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
use std::fs;

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

impl Role {
    fn from_str(s: &str) -> Self {
        match s {
            "Admin" => Role::Admin,
            "Owner" => Role::Owner,
            _ => Role::Member,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ForumConfig {
    #[serde(rename = "category")]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub admins: Vec<String>,
}

pub fn read_forum_config() -> ForumConfig {
    let contents = fs::read_to_string("forum.toml").unwrap_or_else(|_| {
        r#"
        [[category]]
        id = "general"
        name = "General Discussion"
        description = "Default category"
        "#.to_string()
    });
    toml::from_str(&contents).expect("Failed to parse forum.toml")
}

#[derive(Debug, Serialize)]
pub struct Post {
    pub id: String,
    pub category_id: String,
    pub title: String,
    pub content: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub is_locked: bool,
    pub min_reply_role: Role,
    pub view_count: u64,
}

#[derive(sqlx::FromRow)]
struct DbPost {
    id: String,
    category_id: String,
    title: String,
    content: String,
    author: String,
    created_at: DateTime<Utc>,
    updated_at: Option<DateTime<Utc>>,
    is_locked: bool,
    min_reply_role: String,
    view_count: i64,
}

impl From<DbPost> for Post {
    fn from(p: DbPost) -> Self {
        Post {
            id: p.id,
            category_id: p.category_id,
            title: p.title,
            content: p.content,
            author: p.author,
            created_at: p.created_at,
            updated_at: p.updated_at,
            is_locked: p.is_locked,
            min_reply_role: Role::from_str(&p.min_reply_role),
            view_count: p.view_count as u64,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PostView {
    pub id_str: String,
    pub category_id: String,
    pub title: String,
    pub content: String,
    pub author: String,
    pub created_at_str: String,
    pub updated_at_str: Option<String>,
    pub is_locked: bool,
    pub min_reply_role: Role,
    pub view_count: u64,
}

impl From<Post> for PostView {
    fn from(p: Post) -> Self {
        PostView {
            id_str: p.id,
            category_id: p.category_id,
            title: p.title,
            content: p.content,
            author: p.author,
            created_at_str: p.created_at.format("%Y-%m-%d %H:%M").to_string(),
            updated_at_str: p.updated_at.map(|d| d.format("%Y-%m-%d %H:%M").to_string()),
            is_locked: p.is_locked,
            min_reply_role: p.min_reply_role,
            view_count: p.view_count,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReplyView {
    pub id_str: String,
    pub content: String,
    pub author: String,
    pub created_at_str: String,
}

impl From<Reply> for ReplyView {
    fn from(r: Reply) -> Self {
        ReplyView {
            id_str: r.id,
            content: r.content,
            author: r.author,
            created_at_str: r.created_at.format("%Y-%m-%d %H:%M").to_string(),
        }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Reply {
    pub id: String,
    pub thread_id: String,
    pub content: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub is_verified: bool,
    pub verification_token: String,
    pub role: Role,
    pub is_banned: bool,
}

#[derive(sqlx::FromRow)]
struct DbUser {
    username: String,
    email: String,
    password_hash: String,
    is_verified: bool,
    verification_token: String,
    role: String,
    is_banned: bool,
}

impl From<DbUser> for User {
    fn from(u: DbUser) -> Self {
        User {
            username: u.username,
            email: u.email,
            password_hash: u.password_hash,
            is_verified: u.is_verified,
            verification_token: u.verification_token,
            role: Role::from_str(&u.role),
            is_banned: u.is_banned,
        }
    }
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
    pub category_id: String,
    pub title: String,
    pub content: String,
    pub min_reply_role: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateReply {
    pub content: String,
}

#[derive(Deserialize)]
pub struct EditPostForm {
    pub content: String,
}

#[derive(Deserialize)]
pub struct DeleteReplyForm {
    pub thread_id: String,
}

#[derive(Deserialize)]
pub struct CreateCategoryForm {
    pub id: String,
    pub name: String,
    pub description: String,
}

pub type ForumDb = Arc<SqlitePool>;

pub async fn init_db() -> ForumDb {
    let opts = SqliteConnectOptions::new()
        .filename("forum.db")
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS categories (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT NOT NULL
        )"
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            username TEXT PRIMARY KEY,
            email TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            is_verified INTEGER NOT NULL DEFAULT 0,
            verification_token TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'Member',
            is_banned INTEGER NOT NULL DEFAULT 0
        )"
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS posts (
            id TEXT PRIMARY KEY,
            category_id TEXT NOT NULL,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            author TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT,
            is_locked INTEGER NOT NULL DEFAULT 0,
            min_reply_role TEXT NOT NULL DEFAULT 'Member',
            view_count INTEGER NOT NULL DEFAULT 0
        )"
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS replies (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            content TEXT NOT NULL,
            author TEXT NOT NULL,
            created_at TEXT NOT NULL
        )"
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stream_keys (
            username TEXT PRIMARY KEY,
            stream_key TEXT UNIQUE NOT NULL
        )"
    ).execute(&pool).await.unwrap();
    Arc::new(pool)
}

pub async fn seed_categories(db: &ForumDb, config: &ForumConfig) {
    for cat in &config.categories {
        sqlx::query(
            "INSERT OR IGNORE INTO categories (id, name, description) VALUES (?, ?, ?)"
        )
        .bind(&cat.id)
        .bind(&cat.name)
        .bind(&cat.description)
        .execute(&**db)
        .await.ok();
    }
}

pub async fn get_categories(db: &ForumDb) -> Vec<Category> {
    sqlx::query_as::<_, Category>(
        "SELECT id, name, description FROM categories ORDER BY name ASC"
    )
    .fetch_all(&**db)
    .await
    .unwrap_or_default()
}

pub async fn get_current_user(state: &Arc<AppState>, jar: &CookieJar) -> Option<User> {
    let username = jar.get("username")?.value().to_string();
    let row = sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&*state.forum_db)
        .await.ok()??;
    let user = User::from(row);
    if user.is_banned { None } else { Some(user) }
}

fn is_forum_admin(user: &User, forum_config: &ForumConfig) -> bool {
    user.role >= Role::Admin || forum_config.admins.contains(&user.email)
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

pub async fn view_thread(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
    jar: CookieJar
) -> impl IntoResponse {
    let thread = sqlx::query_as::<_, DbPost>("SELECT * FROM posts WHERE id = ?")
        .bind(&thread_id)
        .fetch_optional(&*state.forum_db)
        .await.unwrap_or(None)
        .map(Post::from);

    if thread.is_none() {
        return (StatusCode::NOT_FOUND, "Thread not found").into_response();
    }

    let _ = sqlx::query("UPDATE posts SET view_count = view_count + 1 WHERE id = ?")
        .bind(&thread_id)
        .execute(&*state.forum_db)
        .await;

    let replies: Vec<ReplyView> = sqlx::query_as::<_, Reply>(
        "SELECT * FROM replies WHERE thread_id = ? ORDER BY created_at ASC"
    )
    .bind(&thread_id)
    .fetch_all(&*state.forum_db)
    .await.unwrap_or_default()
    .into_iter().map(ReplyView::from).collect();

    let current_user = get_current_user(&state, &jar).await;
    let is_admin = current_user.as_ref()
        .map(|u| is_forum_admin(u, &state.forum_config))
        .unwrap_or(false);
    let can_reply = if let Some(ref t) = thread {
        if let Some(ref u) = current_user {
            if t.is_locked { is_admin } else { is_admin || u.role >= t.min_reply_role }
        } else {
            false
        }
    } else {
        false
    };
    let thread_created_at_str = thread.as_ref()
        .map(|t| t.created_at.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    let mut context = tera::Context::new();
    context.insert("thread", &thread);
    context.insert("thread_id_str", &thread_id);
    context.insert("thread_created_at_str", &thread_created_at_str);
    context.insert("replies", &replies);
    context.insert("current_user", &current_user);
    context.insert("can_reply", &can_reply);
    context.insert("can_moderate", &is_admin);
    context.insert("is_admin", &is_admin);
    context.insert("base_path", &"/forum");
    match state.tera.render("topic.html", &context) {
        Ok(rendered) => Html(rendered).into_response(),
        Err(e) => {
            eprintln!("Tera error rendering topic.html: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

pub async fn post_reply(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
    jar: CookieJar,
    Form(form): Form<CreateReply>,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to("/forum/login").into_response(),
    };
    let thread = sqlx::query_as::<_, DbPost>("SELECT * FROM posts WHERE id = ?")
        .bind(&thread_id)
        .fetch_optional(&*state.forum_db)
        .await.unwrap_or(None)
        .map(Post::from);

    if let Some(t) = thread {
        if t.is_locked && current_user.role < Role::Admin {
            return (StatusCode::FORBIDDEN, "Thread is locked").into_response();
        }
        if current_user.role < t.min_reply_role {
            return (StatusCode::FORBIDDEN, "Insufficient permissions").into_response();
        }
        let reply_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO replies (id, thread_id, content, author, created_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&reply_id)
        .bind(&t.id)
        .bind(&form.content)
        .bind(&current_user.username)
        .bind(Utc::now())
        .execute(&*state.forum_db)
        .await.unwrap();

        let _ = sqlx::query("UPDATE posts SET updated_at = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(&thread_id)
            .execute(&*state.forum_db)
            .await;

        return Redirect::to(&format!("/forum/thread/{}", thread_id)).into_response();
    }

    (StatusCode::NOT_FOUND, "Thread not found").into_response()
}

pub async fn toggle_lock(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
    jar: CookieJar,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Login required").into_response(),
    };
    if current_user.role < Role::Admin {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let _ = sqlx::query(
        "UPDATE posts SET is_locked = CASE WHEN is_locked = 1 THEN 0 ELSE 1 END WHERE id = ?"
    )
    .bind(&thread_id)
    .execute(&*state.forum_db)
    .await;
    Redirect::to(&format!("/forum/thread/{}", thread_id)).into_response()
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
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&*state.forum_db)
        .await.unwrap_or(0);
    let role = if count == 0 { Role::Owner } else { Role::Member };
    let token = Uuid::new_v4().to_string();
    let hashed = hash(form.password, DEFAULT_COST).unwrap();

    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = ?")
        .bind(&form.username)
        .fetch_one(&*state.forum_db)
        .await.unwrap_or(0);
    if existing > 0 {
        return Html("<h1>Error</h1><p>Username taken.</p>").into_response();
    }

    sqlx::query(
        "INSERT INTO users (username, email, password_hash, is_verified, verification_token, role, is_banned) VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&form.username)
    .bind(&form.email)
    .bind(&hashed)
    .bind(false)
    .bind(&token)
    .bind(role.to_string())
    .bind(false)
    .execute(&*state.forum_db)
    .await.unwrap();

    let _ = send_verification_email(&state.config, &form.email, &token).await;
    Html("<h1>Check your email</h1><p>A verification link has been sent to your email.</p>").into_response()
}

pub async fn verify_email(
    State(state): State<Arc<AppState>>,
    Query(query): Query<VerifyQuery>,
) -> impl IntoResponse {
    let row = sqlx::query_as::<_, DbUser>(
        "SELECT * FROM users WHERE verification_token = ?"
    )
    .bind(&query.token)
    .fetch_optional(&*state.forum_db)
    .await.unwrap_or(None);

    if let Some(db_user) = row {
        let _ = sqlx::query("UPDATE users SET is_verified = 1 WHERE username = ?")
            .bind(&db_user.username)
            .execute(&*state.forum_db)
            .await;
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
    let row = sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE username = ?")
        .bind(&form.username)
        .fetch_optional(&*state.forum_db)
        .await.unwrap_or(None);

    if let Some(db_user) = row {
        let user = User::from(db_user);
        if !user.is_verified {
            return Html("<h1>Please verify your email first</h1>").into_response();
        }
        if user.is_banned {
            return Html("<h1>Your account has been banned from this forum.</h1>").into_response();
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

pub async fn new_post_form(
    State(state): State<Arc<AppState>>,
    Path(category_id): Path<String>,
    jar: CookieJar
) -> impl IntoResponse {
    let base = "/forum";
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to(&format!("{}/login", base)).into_response(),
    };
    if current_user.role < Role::Member {
        return (StatusCode::FORBIDDEN, "Insufficient permissions").into_response();
    }
    let category = get_categories(&state.forum_db).await
        .into_iter().find(|c| c.id == category_id);
    let mut context = tera::Context::new();
    context.insert("category_id", &category_id);
    context.insert("category", &category);
    context.insert("current_user", &current_user);
    context.insert("base_path", &base);
    match state.tera.render("forum_new_topic.html", &context) {
        Ok(rendered) => Html(rendered).into_response(),
        Err(e) => {
            eprintln!("Tera error: {}", e);
            if let Some(source) = std::error::Error::source(&e) {
                eprintln!("Caused by: {}", source);
            }
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
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
    let actual_min_role = if current_user.role >= Role::Admin {
        min_reply_role
    } else {
        Role::Member
    };
    let post_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO posts (id, category_id, title, content, author, created_at, updated_at, is_locked, min_reply_role, view_count) VALUES (?, ?, ?, ?, ?, ?, NULL, 0, ?, 0)"
    )
    .bind(&post_id)
    .bind(&form.category_id)
    .bind(&form.title)
    .bind(&form.content)
    .bind(&current_user.username)
    .bind(Utc::now())
    .bind(actual_min_role.to_string())
    .execute(&*state.forum_db)
    .await.unwrap();
    Redirect::to(&format!("/forum/c/{}", form.category_id)).into_response()
}

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

    let existing = sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE email = ?")
        .bind(&user_info.email)
        .fetch_optional(&*state.forum_db)
        .await.unwrap_or(None);

    let username = if let Some(user) = existing {
        user.username
    } else {
        let base_username = user_info.name.replace(' ', "_");
        let conflict: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = ?")
            .bind(&base_username)
            .fetch_one(&*state.forum_db)
            .await.unwrap_or(0);
        let final_username = if conflict > 0 {
            format!("{}_{}", base_username, &Uuid::new_v4().to_string()[..8])
        } else {
            base_username
        };
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&*state.forum_db)
            .await.unwrap_or(0);
        let role = if count == 0 { Role::Owner } else { Role::Member };
        sqlx::query(
            "INSERT INTO users (username, email, password_hash, is_verified, verification_token, role, is_banned) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&final_username)
        .bind(&user_info.email)
        .bind("GOOGLE_OAUTH")
        .bind(true)
        .bind("google_oauth")
        .bind(role.to_string())
        .bind(false)
        .execute(&*state.forum_db)
        .await.ok();
        final_username
    };

    let cookie = Cookie::build(("username", username))
        .path("/")
        .http_only(true)
        .build();
    (jar.add(cookie), Redirect::to("/forum")).into_response()
}

pub async fn board_index(
    State(state): State<Arc<AppState>>,
    jar: CookieJar
) -> impl IntoResponse {
    let current_user = get_current_user(&state, &jar).await;
    let is_admin = current_user.as_ref()
        .map(|u| is_forum_admin(u, &state.forum_config))
        .unwrap_or(false);
    let categories = get_categories(&state.forum_db).await;
    let mut context = tera::Context::new();
    context.insert("categories", &categories);
    context.insert("current_user", &current_user);
    context.insert("is_admin", &is_admin);
    context.insert("base_path", &"/forum");
    match state.tera.render("forum_index.html", &context) {
        Ok(rendered) => Html(rendered).into_response(),
        Err(e) => {
            eprintln!("Tera error rendering forum_index.html: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

pub async fn view_category(
    State(state): State<Arc<AppState>>,
    Path(category_id): Path<String>,
    jar: CookieJar
) -> impl IntoResponse {
    let posts: Vec<PostView> = sqlx::query_as::<_, DbPost>(
        "SELECT * FROM posts WHERE category_id = ? ORDER BY created_at DESC"
    )
    .bind(&category_id)
    .fetch_all(&*state.forum_db)
    .await.unwrap_or_default()
    .into_iter()
    .map(Post::from)
    .map(PostView::from)
    .collect();

    let category = get_categories(&state.forum_db).await
        .into_iter().find(|c| c.id == category_id);
    let current_user = get_current_user(&state, &jar).await;
    let is_admin = current_user.as_ref()
        .map(|u| is_forum_admin(u, &state.forum_config))
        .unwrap_or(false);
    let mut context = tera::Context::new();
    context.insert("category", &category);
    context.insert("posts", &posts);
    context.insert("current_user", &current_user);
    context.insert("is_admin", &is_admin);
    context.insert("base_path", &"/forum");
    match state.tera.render("forum_category.html", &context) {
        Ok(rendered) => Html(rendered).into_response(),
        Err(e) => {
            eprintln!("Tera error rendering forum_category.html: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

pub async fn admin_panel(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to("/forum/login").into_response(),
    };
    if !is_forum_admin(&current_user, &state.forum_config) {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let banned_users: Vec<User> = sqlx::query_as::<_, DbUser>(
        "SELECT * FROM users WHERE is_banned = 1"
    )
    .fetch_all(&*state.forum_db)
    .await.unwrap_or_default()
    .into_iter().map(User::from).collect();

    let all_users: Vec<User> = sqlx::query_as::<_, DbUser>(
        "SELECT * FROM users ORDER BY role DESC, username ASC"
    )
    .fetch_all(&*state.forum_db)
    .await.unwrap_or_default()
    .into_iter().map(User::from).collect();

    let categories = get_categories(&state.forum_db).await;
    let is_owner = current_user.role >= Role::Owner;
    let mut context = tera::Context::new();
    context.insert("banned_users", &banned_users);
    context.insert("all_users", &all_users);
    context.insert("categories", &categories);
    context.insert("current_user", &current_user);
    context.insert("is_admin", &true);
    context.insert("is_owner", &is_owner);
    context.insert("base_path", &"/forum");
    match state.tera.render("forum_admin.html", &context) {
        Ok(rendered) => Html(rendered).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
    }
}

pub async fn admin_delete_post(
    State(state): State<Arc<AppState>>,
    Path(post_id): Path<String>,
    jar: CookieJar,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to("/forum/login").into_response(),
    };
    if !is_forum_admin(&current_user, &state.forum_config) {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let category_id: Option<String> = sqlx::query_scalar(
        "SELECT category_id FROM posts WHERE id = ?"
    )
    .bind(&post_id)
    .fetch_optional(&*state.forum_db)
    .await.unwrap_or(None);

    let _ = sqlx::query("DELETE FROM replies WHERE thread_id = ?")
        .bind(&post_id)
        .execute(&*state.forum_db)
        .await;
    let _ = sqlx::query("DELETE FROM posts WHERE id = ?")
        .bind(&post_id)
        .execute(&*state.forum_db)
        .await;
    Redirect::to(&format!("/forum/c/{}", category_id.unwrap_or_default())).into_response()
}

pub async fn admin_delete_reply(
    State(state): State<Arc<AppState>>,
    Path(reply_id): Path<String>,
    jar: CookieJar,
    Form(form): Form<DeleteReplyForm>,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to("/forum/login").into_response(),
    };
    if !is_forum_admin(&current_user, &state.forum_config) {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let _ = sqlx::query("DELETE FROM replies WHERE id = ?")
        .bind(&reply_id)
        .execute(&*state.forum_db)
        .await;
    Redirect::to(&format!("/forum/thread/{}", form.thread_id)).into_response()
}

pub async fn admin_edit_post_form(
    State(state): State<Arc<AppState>>,
    Path(post_id): Path<String>,
    jar: CookieJar,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to("/forum/login").into_response(),
    };
    if !is_forum_admin(&current_user, &state.forum_config) {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let post = sqlx::query_as::<_, DbPost>("SELECT * FROM posts WHERE id = ?")
        .bind(&post_id)
        .fetch_optional(&*state.forum_db)
        .await.unwrap_or(None)
        .map(Post::from);

    let mut context = tera::Context::new();
    context.insert("post", &post);
    context.insert("post_id_str", &post_id);
    context.insert("current_user", &current_user);
    context.insert("is_admin", &true);
    context.insert("base_path", &"/forum");
    match state.tera.render("forum_edit_post.html", &context) {
        Ok(rendered) => Html(rendered).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
    }
}

pub async fn admin_edit_post(
    State(state): State<Arc<AppState>>,
    Path(post_id): Path<String>,
    jar: CookieJar,
    Form(form): Form<EditPostForm>,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to("/forum/login").into_response(),
    };
    if !is_forum_admin(&current_user, &state.forum_config) {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let _ = sqlx::query(
        "UPDATE posts SET content = ?, updated_at = ? WHERE id = ?"
    )
    .bind(&form.content)
    .bind(Utc::now())
    .bind(&post_id)
    .execute(&*state.forum_db)
    .await;
    Redirect::to(&format!("/forum/thread/{}", post_id)).into_response()
}

#[derive(Deserialize)]
pub struct SetRoleForm {
    pub role: String,
}

pub async fn admin_set_role(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    jar: CookieJar,
    Form(form): Form<SetRoleForm>,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Login required").into_response(),
    };
    if current_user.role < Role::Owner {
        return (StatusCode::FORBIDDEN, "Only the Owner can change roles").into_response();
    }
    if current_user.username == username {
        return (StatusCode::BAD_REQUEST, "Cannot change your own role").into_response();
    }
    let new_role = match form.role.as_str() {
        "Admin" => Role::Admin,
        "Owner" => Role::Owner,
        _ => Role::Member,
    };
    let _ = sqlx::query("UPDATE users SET role = ? WHERE username = ?")
        .bind(new_role.to_string())
        .bind(&username)
        .execute(&*state.forum_db)
        .await;
    Redirect::to("/forum/admin").into_response()
}

pub async fn admin_ban_user(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    jar: CookieJar,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Login required").into_response(),
    };
    if !is_forum_admin(&current_user, &state.forum_config) {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let target = sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&*state.forum_db)
        .await.unwrap_or(None)
        .map(User::from);

    if let Some(ref t) = target {
        if is_forum_admin(t, &state.forum_config) {
            return (StatusCode::FORBIDDEN, "Cannot ban an admin").into_response();
        }
    }
    let _ = sqlx::query("UPDATE users SET is_banned = 1 WHERE username = ?")
        .bind(&username)
        .execute(&*state.forum_db)
        .await;
    Redirect::to("/forum/admin").into_response()
}

pub async fn admin_unban_user(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    jar: CookieJar,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Login required").into_response(),
    };
    if !is_forum_admin(&current_user, &state.forum_config) {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let _ = sqlx::query("UPDATE users SET is_banned = 0 WHERE username = ?")
        .bind(&username)
        .execute(&*state.forum_db)
        .await;
    Redirect::to("/forum/admin").into_response()
}

pub async fn admin_add_category(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(form): Form<CreateCategoryForm>,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return Redirect::to("/forum/login").into_response(),
    };
    if !is_forum_admin(&current_user, &state.forum_config) {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let slug: String = form.id.trim().to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        return (StatusCode::BAD_REQUEST, "Category ID cannot be empty").into_response();
    }
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories WHERE id = ?")
        .bind(&slug)
        .fetch_one(&*state.forum_db)
        .await.unwrap_or(0);
    if existing > 0 {
        return (StatusCode::CONFLICT, "A category with this ID already exists").into_response();
    }
    sqlx::query("INSERT INTO categories (id, name, description) VALUES (?, ?, ?)")
        .bind(&slug)
        .bind(&form.name)
        .bind(&form.description)
        .execute(&*state.forum_db)
        .await.ok();
    Redirect::to("/forum/admin").into_response()
}

pub async fn admin_delete_category(
    State(state): State<Arc<AppState>>,
    Path(category_id): Path<String>,
    jar: CookieJar,
) -> impl IntoResponse {
    let current_user = match get_current_user(&state, &jar).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Login required").into_response(),
    };
    if !is_forum_admin(&current_user, &state.forum_config) {
        return (StatusCode::FORBIDDEN, "Admin access required").into_response();
    }
    let _ = sqlx::query(
        "DELETE FROM replies WHERE thread_id IN (SELECT id FROM posts WHERE category_id = ?)"
    )
    .bind(&category_id)
    .execute(&*state.forum_db)
    .await;
    let _ = sqlx::query("DELETE FROM posts WHERE category_id = ?")
        .bind(&category_id)
        .execute(&*state.forum_db)
        .await;
    let _ = sqlx::query("DELETE FROM categories WHERE id = ?")
        .bind(&category_id)
        .execute(&*state.forum_db)
        .await;
    Redirect::to("/forum/admin").into_response()
}
