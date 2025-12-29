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
    pub password_hash: String,
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

pub async fn list_posts(State(db): State<ForumDb>, jar: CookieJar) -> impl IntoResponse {
    let posts: Vec<Post> = db.select("posts").await.unwrap_or_default();
    let current_user = get_user(&jar);
    
    let auth_links = if let Some(user) = &current_user {
        format!("Logged in as: <strong>{}</strong> | <a href='/forum/logout'>Logout</a>", user)
    } else {
        String::from("<a href='/forum/login'>Login</a> | <a href='/forum/register'>Register</a>")
    };
    let mut html = format!(r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>Forum</title>
            <link rel="stylesheet" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css">
        </head>
        <body style="max-width: 800px; margin: 0 auto; padding: 20px;">
            <h1>Webify Forum</h1>
            <a href="/">Home</a> | {} | <a href="/forum/new">New Post</a>
            <hr>
    "#, auth_links);
    for post in posts.iter().rev() {
        html.push_str(&format!(
            "<div><h3>{}</h3><p>By {} on {}</p><p>{}</p></div><hr>",
            post.title, post.author, post.created_at.format("%Y-%m-%d %H:%M"), post.content
        ));
    }
    html.push_str("</body></html>");
    Html(html)
}

pub async fn register_form() -> Html<String> {
    Html(r#"
        <!DOCTYPE html>
        <html>
        <head><link rel="stylesheet" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></head>
        <body style="max-width: 400px; margin: 50px auto; padding: 20px; border: 1px solid #586e75;">
            <h1>Register</h1>
            <form action="/forum/register" method="post">
                <input type="text" name="username" placeholder="Username" style="width:100%" required><br><br>
                <input type="password" name="password" placeholder="Password" style="width:100%" required><br><br>
                <button type="submit" style="width:100%">Create Account</button>
            </form>
            <p><a href="/forum/login">Already have an account? Login</a></p>
        </body>
        </html>
    "#.to_string())
}

#[derive(Deserialize)]
pub struct AuthForm {
    pub username: String,
    pub password: String,
}

pub async fn register(State(db): State<ForumDb>, Form(form): Form<AuthForm>) -> impl IntoResponse {
    let hashed = hash(form.password, DEFAULT_COST).unwrap();
    let user = User { username: form.username.clone(), password_hash: hashed };
    let _: Option<User> = db.create(("users", &form.username)).content(user).await.unwrap();
    Redirect::to("/forum/login")
}

pub async fn login_form() -> Html<String> {
    Html(r#"
        <!DOCTYPE html>
        <html>
        <head><link rel="stylesheet" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></head>
        <body style="max-width: 400px; margin: 50px auto; padding: 20px; border: 1px solid #586e75;">
            <h1>Login</h1>
            <form action="/forum/login" method="post">
                <input type="text" name="username" placeholder="Username" style="width:100%" required><br><br>
                <input type="password" name="password" placeholder="Password" style="width:100%" required><br><br>
                <button type="submit" style="width:100%">Login</button>
            </form>
            <p><a href="/forum/register">No account? Register</a></p>
        </body>
        </html>
    "#.to_string())
}

pub async fn login(State(db): State<ForumDb>, jar: CookieJar, Form(form): Form<AuthForm>) -> impl IntoResponse {
    let user: Option<User> = db.select(("users", &form.username)).await.unwrap();
    if let Some(user) = user {
        if verify(form.password, &user.password_hash).unwrap() {
            let cookie = Cookie::build(("username", user.username))
                .path("/")
                .http_only(true)
                .build();
            return (jar.add(cookie), Redirect::to("/forum")).into_response();
        }
    }
    Html("<h1>Invalid Credentials</h1><a href='/forum/login'>Try again</a>").into_response()
}

pub async fn logout(jar: CookieJar) -> impl IntoResponse {
    (jar.remove(Cookie::from("username")), Redirect::to("/forum"))
}

pub async fn new_post_form(jar: CookieJar) -> impl IntoResponse {
    if get_user(&jar).is_none() {
        return Redirect::to("/forum/login").into_response();
    }
    Html(r#"
        <!DOCTYPE html>
        <html>
        <head><link rel="stylesheet" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></head>
        <body style="max-width: 800px; margin: 0 auto; padding: 20px;">
            <h1>Create New Post</h1>
            <form action="/forum/create" method="post">
                <input type="text" name="title" placeholder="Title" required><br><br>
                <textarea name="content" placeholder="Content" rows="10" style="width:100%" required></textarea><br><br>
                <button type="submit">Post</button>
            </form>
        </body>
        </html>
    "#).into_response()
}

#[derive(Deserialize)]
pub struct CreatePost {
    pub title: String,
    pub content: String,
}

pub async fn create_post(
    State(db): State<ForumDb>,
    jar: CookieJar,
    Form(form): Form<CreatePost>,
) -> impl IntoResponse {
    let Some(username) = get_user(&jar) else {
        return Redirect::to("/forum/login").into_response();
    };
    let _: Option<Post> = db.create("posts")
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
