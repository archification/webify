use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use serde::{Deserialize, Serialize};
use axum::{
    extract::{State, Form},
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use bcrypt::{hash, verify, DEFAULT_COST};
use tokio::fs;

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

pub type ForumDb = Arc<Surreal<Db>>;

#[derive(Clone)]
pub struct ForumState {
    pub db: ForumDb,
    pub template_path: String,
    pub base_path: String,
}

pub async fn init_db() -> ForumDb {
    let db = Surreal::new::<surrealdb::engine::local::RocksDb>("forum.db").await.unwrap();
    db.use_ns("webify").use_db("forum").await.unwrap();
    Arc::new(db)
}

fn get_user(jar: &CookieJar) -> Option<String> {
    jar.get("username").map(|c| c.value().to_string())
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
            "<a href='/'>Home</a> | <a href='{}/login'>Login</a> | <a href='{}/register'>Register</a> | <a href='{}/new'>New Post</a>",
            base, base, base
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
    let auth_tag = "{{AUTH}}";
    let post_tag = "{{POSTS}}";
    if html.contains(auth_tag) {
        html = html.replace(auth_tag, &auth_links);
    }
    if html.contains(post_tag) {
        html = html.replace(post_tag, &posts_html);
    } else {
        html.push_str("<hr><h2>Posts</h2>");
        html.push_str(&posts_html);
    }
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
        return Html(format!(
            "<h1>Error</h1><p>Passwords do not match.</p><a href='{}/register'>Try again</a>",
            state.base_path.trim_end_matches('/')
        )).into_response();
    }
    let hashed = hash(form.password, DEFAULT_COST).unwrap();
    let user = User { username: form.username.clone(), email: form.email, password_hash: hashed };
    let _: Option<User> = state.db.create(("users", &form.username)).content(user).await.unwrap();
    let redirect_url = format!("{}/login", state.base_path.trim_end_matches('/'));
    Redirect::to(&redirect_url).into_response()
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
        if verify(form.password, &user.password_hash).unwrap() {
            let cookie = Cookie::build(("username", user.username))
                .path("/")
                .http_only(true)
                .build();
            let redirect_url = format!("{}", state.base_path);
            return (jar.add(cookie), Redirect::to(&redirect_url)).into_response();
        }
    }
    Html(format!(
        "<h1>Invalid Credentials</h1><a href='{}/login'>Try again</a>",
        state.base_path.trim_end_matches('/')
    )).into_response()
}

pub async fn logout(State(state): State<Arc<ForumState>>, jar: CookieJar) -> impl IntoResponse {
    let redirect_url = format!("{}", state.base_path);
    (jar.remove(Cookie::from("username")), Redirect::to(&redirect_url))
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
