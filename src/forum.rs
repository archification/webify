use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use serde::{Deserialize, Serialize};
use axum::{
    extract::{State, Form, Query},
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Post {
    pub title: String,
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
//    pub state: String,
}

#[derive(Deserialize, Debug)]
pub struct GoogleUser {
//    pub id: String,
    pub email: String,
//    pub verified_email: bool,
    pub name: String,
//    pub picture: String,
}

pub type ForumDb = Arc<Surreal<Db>>;

pub async fn init_db() -> ForumDb {
    let db = Surreal::new::<surrealdb::engine::local::RocksDb>("forum.db").await.unwrap();
    db.use_ns("webify").use_db("forum").await.unwrap();
    Arc::new(db)
}

fn get_user(jar: &CookieJar) -> Option<String> {
    jar.get("username").map(|c| c.value().to_string())
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

pub async fn list_posts(
    State(state): State<Arc<AppState>>,
    jar: CookieJar
) -> impl IntoResponse {
    let posts: Vec<Post> = state.forum_db.select("posts").await.unwrap_or_default();
    let current_user = get_user(&jar);
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
    let token = Uuid::new_v4().to_string();
    let hashed = hash(form.password, DEFAULT_COST).unwrap();
    let user = User { 
        username: form.username.clone(), 
        email: form.email.clone(), 
        password_hash: hashed,
        is_verified: false,
        verification_token: token.clone(),
    };
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

pub async fn new_post_form(State(_state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    let base = "/forum";
    if get_user(&jar).is_none() {
        return Redirect::to(&format!("{}/login", base)).into_response();
    }
    Html(format!(r#"
        <!DOCTYPE html>
        <html>
        <head><link rel="stylesheet" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></head>
        <body style="max-width: 800px; margin: 0 auto; padding: 20px;">
            <h1>Create New Post</h1>
            <form action="{}/create" method="post">
                <input type="text" name="title" placeholder="Title" required><br><br>
                <textarea name="content" placeholder="Content" rows="10" style="width:100%" required></textarea><br><br>
                <button type="submit">Post</button>
            </form>
        </body>
        </html>
    "#, base)).into_response()
}

#[derive(Deserialize)]
pub struct CreatePost {
    pub title: String,
    pub content: String,
}

pub async fn create_post(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(form): Form<CreatePost>,
) -> impl IntoResponse {
    let Some(username) = get_user(&jar) else {
        return Redirect::to("/forum/login").into_response();
    };
    let _: Option<Post> = state.forum_db.create("posts")
        .content(Post {
            title: form.title,
            content: form.content,
            author: username,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    Redirect::to("/forum").into_response()
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
    let mut result = state.forum_db
        .query("SELECT * FROM users WHERE email = $email")
        .bind(("email", user_info.email.clone()))
        .await
        .unwrap();
    let users: Vec<User> = result.take(0).unwrap_or_default();
    let username = if let Some(user) = users.first() {
        user.username.clone()
    } else {
        let base_username = user_info.name.replace(' ', "_");
        let user = User {
            username: base_username.clone(),
            email: user_info.email.clone(),
            password_hash: "GOOGLE_OAUTH".to_string(),
            is_verified: true,
            verification_token: "google_oauth".to_string(),
        };
        match state.forum_db.create::<Option<User>>(("users", &base_username)).content(user.clone()).await {
            Ok(_) => base_username,
            Err(_) => {
                 let fallback_username = format!("{}_{}", base_username, Uuid::new_v4().to_string().split('-').next().unwrap());
                 let _ : Option<User> = state.forum_db.create(("users", &fallback_username)).content(user).await.unwrap();
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
