use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use serde::{Deserialize, Serialize};
use axum::{
    extract::{State, Form, Query},
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use bcrypt::{hash, verify, DEFAULT_COST};
use tokio::fs;
use uuid::Uuid;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

#[derive(Debug, Serialize, Deserialize)]
pub struct Post {
    pub title: String,
    pub content: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
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

pub type ForumDb = Arc<Surreal<Db>>;

#[derive(Clone)]
pub struct ForumState {
    pub db: ForumDb,
    pub template_path: String,
    pub base_path: String,
    pub config: Arc<crate::config::Config>,
}

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

pub async fn list_posts(State(state): State<Arc<ForumState>>, jar: CookieJar) -> impl IntoResponse {
    let posts: Vec<Post> = state.db.select("posts").await.unwrap_or_default();
    let current_user = get_user(&jar);
    let base = state.base_path.trim_end_matches('/');
    
    let auth_links = if let Some(user) = &current_user {
        format!(
            "<a href='/'>Home</a> | Logged in as: <strong>{}</strong> | <a href='{}/logout'>Logout</a> | <a href='{}/new'>New Post</a>",
            user, base, base
        )
    } else {
        format!(
            "<a href='/'>Home</a> | <a href='{}/login'>Login</a> | <a href='{}/register'>Register</a>",
            base, base
        )
    };

    let mut posts_html = String::new();
    for post in posts.iter().rev() {
        posts_html.push_str(&format!(
            "<div><h3>{}</h3><p>By {} on {}</p><p>{}</p></div><hr>",
            post.title, post.author, post.created_at.format("%Y-%m-%d %H:%M"), post.content
        ));
    }

    let mut html = fs::read_to_string(&state.template_path).await.unwrap_or_else(|_| {
        format!("<h1>Error: Template not found at {}</h1>", state.template_path)
    });

    if html.contains("{{AUTH}}") { html = html.replace("{{AUTH}}", &auth_links); }
    if html.contains("{{POSTS}}") { html = html.replace("{{POSTS}}", &posts_html); }

    Html(html)
}

pub async fn register_form(State(state): State<Arc<ForumState>>) -> Html<String> {
    let base = state.base_path.trim_end_matches('/');
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
    State(state): State<Arc<ForumState>>, 
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

    let _: Option<User> = state.db.create(("users", &form.username)).content(user).await.unwrap();
    let _ = send_verification_email(&state.config, &form.email, &token).await;

    Html("<h1>Check your email</h1><p>A verification link has been sent to your email.</p>").into_response()
}

pub async fn verify_email(
    State(state): State<Arc<ForumState>>,
    Query(query): Query<VerifyQuery>,
) -> impl IntoResponse {
    println!("Verifying token: [{}]", query.token);
    let mut result = state.db
        .query("SELECT * FROM users WHERE verification_token = $v_token")
        .bind(("v_token", query.token)) 
        .await
        .expect("Verification query failed"); 
    let users: Vec<User> = result.take(0).unwrap_or_default();
    if let Some(user) = users.into_iter().next() {
        let _: Option<User> = state.db.update(("users", &user.username))
            .merge(serde_json::json!({ "is_verified": true }))
            .await
            .expect("Failed to update user status");
        Html("<h1>Verified!</h1><p>Your account is now active. <a href='/forum/login'>Login here</a>.</p>").into_response()
    } else {
        Html("<h1>Invalid link</h1><p>This verification link is invalid or has expired.</p>").into_response()
    }
}

pub async fn login_form(State(state): State<Arc<ForumState>>) -> Html<String> {
    let base = state.base_path.trim_end_matches('/');
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
            <p><a href="{}/register">No account? Register</a></p>
        </body>
        </html>
    "#, base, base))
}

pub async fn login(
    State(state): State<Arc<ForumState>>, 
    jar: CookieJar, 
    Form(form): Form<LoginForm>
) -> impl IntoResponse {
    let user: Option<User> = state.db.select(("users", &form.username)).await.unwrap();
    if let Some(user) = user {
        if !user.is_verified {
            return Html("<h1>Please verify your email first</h1>").into_response();
        }
        if verify(form.password, &user.password_hash).unwrap() {
            let cookie = Cookie::build(("username", user.username)).path("/").http_only(true).build();
            return (jar.add(cookie), Redirect::to(&state.base_path)).into_response();
        }
    }
    Html("<h1>Invalid Credentials</h1>").into_response()
}

pub async fn logout(State(state): State<Arc<ForumState>>, jar: CookieJar) -> impl IntoResponse {
    let cookie = Cookie::build("username").path("/").build();
    (jar.remove(cookie), Redirect::to(&state.base_path))
}

pub async fn new_post_form(State(state): State<Arc<ForumState>>, jar: CookieJar) -> impl IntoResponse {
    let base = state.base_path.trim_end_matches('/');
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
    State(state): State<Arc<ForumState>>,
    jar: CookieJar,
    Form(form): Form<CreatePost>,
) -> impl IntoResponse {
    let base = state.base_path.trim_end_matches('/');
    let Some(username) = get_user(&jar) else {
        return Redirect::to(&format!("{}/login", base)).into_response();
    };
    let _: Option<Post> = state.db.create("posts")
        .content(Post {
            title: form.title,
            content: form.content,
            author: username,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    Redirect::to(&format!("{}", state.base_path)).into_response()
}
