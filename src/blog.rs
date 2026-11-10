use axum::{
    extract::{Path as AxumPath, Query, State, Multipart},
    response::{Html, IntoResponse, Redirect, Json},
    http::{HeaderMap, StatusCode, header},
};
use serde::Serialize;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;
use chrono::Utc;

use crate::AppState;
use crate::auth_guard;
use crate::forum::ForumDb;
use crate::roles;

const POSTS_DIR: &str = "static/posts";
const DRAFTS_DIR: &str = "drafts";
const IMAGES_DIR: &str = "static/images/blog";

// ─── Slug / front-matter helpers ─────────────────────────────────────────────

/// Turn a human title into a url-safe slug: lowercase, non-alphanumerics → '-',
/// collapsed and trimmed.
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in title.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() { "untitled".to_string() } else { slug }
}

/// Inverse-ish of slugify for display fallback when a file has no title front matter.
fn deslugify(slug: &str) -> String {
    slug.replace(['-', '_'], " ")
}

#[derive(Default, Clone)]
pub struct FrontMatter {
    pub title: Option<String>,
    pub date: Option<String>,
    pub image: Option<String>,
}

/// Parse an optional leading `---` YAML-ish front-matter block. Returns the parsed
/// metadata and the remaining markdown body. Files without a block (legacy posts)
/// yield an empty FrontMatter and the whole content as the body.
pub fn parse_front_matter(content: &str) -> (FrontMatter, String) {
    let mut fm = FrontMatter::default();
    let trimmed = content.trim_start_matches('\u{feff}');
    if let Some(rest) = trimmed.strip_prefix("---") {
        // Body starts after the next line that is exactly "---".
        if let Some(end) = rest.find("\n---") {
            let header = &rest[..end];
            // Skip past the closing "---" line.
            let after = &rest[end + 4..];
            let body = after.strip_prefix('\n').unwrap_or(after);
            for line in header.lines() {
                let line = line.trim();
                if let Some((key, val)) = line.split_once(':') {
                    let val = val.trim().trim_matches('"').trim().to_string();
                    if val.is_empty() { continue; }
                    match key.trim() {
                        "title" => fm.title = Some(val),
                        "date" => fm.date = Some(val),
                        "image" => fm.image = Some(val),
                        _ => {}
                    }
                }
            }
            return (fm, body.to_string());
        }
    }
    (fm, content.to_string())
}

/// Re-serialize front matter + body into the on-disk markdown format.
fn build_markdown(title: &str, date: &str, image: Option<&str>, body: &str) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("title: {}\n", title.replace('\n', " ")));
    out.push_str(&format!("date: {}\n", date));
    if let Some(img) = image {
        if !img.is_empty() {
            out.push_str(&format!("image: {}\n", img));
        }
    }
    out.push_str("---\n\n");
    out.push_str(body.trim());
    out.push('\n');
    out
}

// ─── Timestamps ──────────────────────────────────────────────────────────────

/// Current UTC instant as a stable, second-precision RFC 3339 string
/// (e.g. `2026-06-04T14:30:22Z`). This is what we persist — unambiguous and
/// universally interpretable regardless of where the server or reader lives.
pub fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Human-readable UTC rendering used as the no-JS fallback text inside a `<time>`
/// element; the client then rewrites it to the viewer's local timezone.
pub fn humanize_date(iso: &str) -> Option<String> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(iso) {
        return Some(dt.format("%B %-d, %Y").to_string());
    }
    // Legacy posts may carry a date-only value (e.g. "2026-06-04").
    if let Ok(d) = chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d") {
        return Some(d.format("%B %-d, %Y").to_string());
    }
    None
}

#[derive(Serialize, Clone)]
pub struct PostMeta {
    pub slug: String,
    pub title: String,
    pub image: Option<String>,
    /// RFC 3339 UTC instant the post was created (for the `datetime` attribute).
    pub date: Option<String>,
    /// Human-readable UTC fallback text shown before client-side localization.
    pub date_display: Option<String>,
    /// Whether the current viewer may edit this post (set per request).
    pub editable: bool,
}

/// List every `.md` file in `dir` as PostMeta, newest first (by date, then title).
async fn list_dir(dir: &str) -> Vec<PostMeta> {
    let mut out = Vec::new();
    let mut entries = match fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return out,
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let slug = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let content = fs::read_to_string(&path).await.unwrap_or_default();
        let (fm, _body) = parse_front_matter(&content);
        let title = fm.title.unwrap_or_else(|| deslugify(&slug));
        let date_display = fm.date.as_deref().and_then(humanize_date);
        out.push(PostMeta { slug, title, image: fm.image, date: fm.date, date_display, editable: false });
    }
    out.sort_by(|a, b| {
        b.date.cmp(&a.date).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    out
}

// ─── Auth helpers ────────────────────────────────────────────────────────────

fn host_from_headers(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.split(':').next())
        .unwrap_or("")
        .to_string()
}

/// Who is looking at a blog page, as far as authoring goes.
pub struct Viewer {
    pub email: Option<String>,
    /// Holds the writer role (owners always do).
    pub writer: bool,
    /// Dashboard owner from config.toml: may edit every post.
    pub owner: bool,
}

impl Viewer {
    /// Owners edit anything; writers edit only posts recorded as theirs. Posts
    /// with no recorded author (legacy files, or ones rsynced in) are owner-only.
    pub fn can_edit(&self, author: Option<&str>) -> bool {
        if self.owner {
            return true;
        }
        match (&self.email, author) {
            (Some(e), Some(a)) => self.writer && e.eq_ignore_ascii_case(a),
            _ => false,
        }
    }
}

pub async fn viewer(state: &Arc<AppState>, headers: &HeaderMap) -> Viewer {
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let email = match auth_guard::extract_cookie_value(cookie_header, auth_guard::GUARD_COOKIE) {
        Some(t) => auth_guard::validate_session(&state.forum_db, &t).await,
        None => None,
    };
    let Some(email) = email else {
        return Viewer { email: None, writer: false, owner: false };
    };
    let owner = roles::is_owner(&state.config, &email);
    let writer = roles::has_role(&state.config, &state.forum_db, &email, roles::WRITER).await;
    Viewer { email: Some(email), writer, owner }
}

/// Gate for every authoring route: sends signed-out visitors to the login page
/// (returning to `next`) and refuses signed-in users without the writer role.
async fn require_writer(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    next: &str,
) -> Result<Viewer, axum::response::Response> {
    let v = viewer(state, headers).await;
    if v.email.is_none() {
        let url = format!(
            "/auth/login?next={}&host={}",
            urlencoding::encode(next),
            urlencoding::encode(&host_from_headers(headers)),
        );
        return Err(Redirect::to(&url).into_response());
    }
    if !v.writer {
        return Err((StatusCode::FORBIDDEN, "You need the Writer role to write blog posts").into_response());
    }
    Ok(v)
}

pub async fn init_tables(db: &ForumDb) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS blog_authors (
            slug TEXT PRIMARY KEY,
            email TEXT NOT NULL
        )"
    ).execute(&**db).await.unwrap();
}

/// Email of the writer who created `slug`, if one was recorded.
pub async fn post_author(db: &ForumDb, slug: &str) -> Option<String> {
    sqlx::query_scalar("SELECT email FROM blog_authors WHERE slug = ?")
        .bind(slug)
        .fetch_optional(&**db)
        .await
        .unwrap_or(None)
}

async fn set_post_author(db: &ForumDb, slug: &str, email: &str) {
    let _ = sqlx::query("INSERT OR REPLACE INTO blog_authors (slug, email) VALUES (?, ?)")
        .bind(slug)
        .bind(email.to_ascii_lowercase())
        .execute(&**db)
        .await;
}

/// Follow a post to its new slug, dropping any stale row already there.
async fn move_post_author(db: &ForumDb, from: &str, to: &str) {
    if from == to {
        return;
    }
    let _ = sqlx::query("DELETE FROM blog_authors WHERE slug = ?").bind(to).execute(&**db).await;
    let _ = sqlx::query("UPDATE blog_authors SET slug = ? WHERE slug = ?")
        .bind(to)
        .bind(from)
        .execute(&**db)
        .await;
}

// ─── Read handlers ───────────────────────────────────────────────────────────

/// GET /blog — public index. Lists published posts; also lists drafts for writers.
pub async fn blog_index(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let v = viewer(&state, &headers).await;
    let can_edit = v.writer;
    let mut posts = list_dir(POSTS_DIR).await;
    for p in &mut posts {
        p.editable = v.can_edit(post_author(&state.forum_db, &p.slug).await.as_deref());
    }
    // Writers only see their own drafts; owners see everyone's.
    let mut drafts = Vec::new();
    if can_edit {
        for mut d in list_dir(DRAFTS_DIR).await {
            if v.can_edit(post_author(&state.forum_db, &d.slug).await.as_deref()) {
                d.editable = true;
                drafts.push(d);
            }
        }
    }

    let mut ctx = tera::Context::new();
    ctx.insert("port", &state.config.port);
    ctx.insert("domain", &state.config.domain);
    ctx.insert("posts", &posts);
    ctx.insert("drafts", &drafts);
    ctx.insert("can_edit", &can_edit);

    match state.tera.render("blog.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            eprintln!("blog index render error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        }
    }
}

fn render_editor(state: &Arc<AppState>, ctx: tera::Context) -> axum::response::Response {
    match state.tera.render("blog-editor.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            eprintln!("blog editor render error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
        }
    }
}

/// GET /blog/new — empty authoring form (writer role required).
pub async fn new_post_form(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = require_writer(&state, &headers, "/blog/new").await {
        return r;
    }
    let mut ctx = tera::Context::new();
    ctx.insert("port", &state.config.port);
    ctx.insert("domain", &state.config.domain);
    ctx.insert("mode", "new");
    ctx.insert("form_action", "/blog/create");
    ctx.insert("title_value", "");
    ctx.insert("body_value", "");
    ctx.insert("image_value", "");
    ctx.insert("orig_slug", "");
    ctx.insert("orig_status", "");
    render_editor(&state, ctx)
}

/// GET /blog/edit/{slug} — pre-filled authoring form for an existing post or draft.
pub async fn edit_post_form(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
    AxumPath(slug): AxumPath<String>,
) -> impl IntoResponse {
    let slug = sanitize_filename::sanitize(&slug);
    let v = match require_writer(&state, &headers, &format!("/blog/edit/{}", slug)).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !v.can_edit(post_author(&state.forum_db, &slug).await.as_deref()) {
        return (StatusCode::FORBIDDEN, "You can only edit your own posts").into_response();
    }
    let (content, status) = match fs::read_to_string(format!("{}/{}.md", POSTS_DIR, slug)).await {
        Ok(c) => (c, "published"),
        Err(_) => match fs::read_to_string(format!("{}/{}.md", DRAFTS_DIR, slug)).await {
            Ok(c) => (c, "draft"),
            Err(_) => return (StatusCode::NOT_FOUND, "Post not found").into_response(),
        },
    };
    let (fm, body) = parse_front_matter(&content);
    let title = fm.title.unwrap_or_else(|| deslugify(&slug));

    let mut ctx = tera::Context::new();
    ctx.insert("port", &state.config.port);
    ctx.insert("domain", &state.config.domain);
    ctx.insert("mode", "edit");
    ctx.insert("form_action", "/blog/update");
    ctx.insert("title_value", &title);
    ctx.insert("body_value", &body);
    ctx.insert("image_value", &fm.image.unwrap_or_default());
    ctx.insert("orig_slug", &slug);
    ctx.insert("orig_status", status);
    ctx.insert("saved", &q.contains_key("saved"));
    render_editor(&state, ctx)
}

// ─── Write handlers ──────────────────────────────────────────────────────────

#[derive(Default)]
struct PostForm {
    title: String,
    body: String,
    action: String,
    orig_slug: String,
    orig_status: String,
    image_bytes: Option<Vec<u8>>,
    image_filename: Option<String>,
}

async fn parse_post_form(mut multipart: Multipart) -> Result<PostForm, String> {
    let mut form = PostForm::default();
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return Err(format!("multipart error: {}", e)),
        };
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "image" => {
                let filename = field.file_name().map(|s| s.to_string());
                let data = field.bytes().await.map_err(|e| e.to_string())?;
                if let Some(fname) = filename {
                    if !fname.is_empty() && !data.is_empty() {
                        form.image_bytes = Some(data.to_vec());
                        form.image_filename = Some(fname);
                    }
                }
            }
            "title" => form.title = field.text().await.unwrap_or_default(),
            "body" => form.body = field.text().await.unwrap_or_default(),
            "action" => form.action = field.text().await.unwrap_or_default(),
            "orig_slug" => form.orig_slug = field.text().await.unwrap_or_default(),
            "orig_status" => form.orig_status = field.text().await.unwrap_or_default(),
            _ => { let _ = field.bytes().await; }
        }
    }
    Ok(form)
}

/// Save uploaded image bytes to static/images/blog and return its public URL.
async fn save_image(slug: &str, bytes: &[u8], original: &str) -> Option<String> {
    let ext: String = Path::new(original)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.chars().filter(|c| c.is_ascii_alphanumeric()).collect())
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| "img".to_string());
    if fs::create_dir_all(IMAGES_DIR).await.is_err() {
        return None;
    }
    let filename = format!("{}-{}.{}", slug, Uuid::new_v4(), ext.to_ascii_lowercase());
    let path = format!("{}/{}", IMAGES_DIR, filename);
    fs::write(&path, bytes).await.ok()?;
    Some(format!("/{}/{}", IMAGES_DIR, filename))
}

/// Find a slug not already used by a file in any of `dirs`.
async fn ensure_unique_slug(base: &str, dirs: &[&str]) -> String {
    let mut slug = base.to_string();
    let mut n = 2;
    loop {
        let mut taken = false;
        for dir in dirs {
            if fs::try_exists(format!("{}/{}.md", dir, slug)).await.unwrap_or(false) {
                taken = true;
                break;
            }
        }
        if !taken {
            return slug;
        }
        slug = format!("{}-{}", base, n);
        n += 1;
    }
}

/// Map a stored status string to its on-disk directory ("" if unknown).
fn status_dir(status: &str) -> &'static str {
    match status {
        "draft" => DRAFTS_DIR,
        "published" => POSTS_DIR,
        _ => "",
    }
}

/// Read the front matter of an existing post/draft (by status + slug), if present.
async fn read_prior_fm(status: &str, slug: &str) -> Option<FrontMatter> {
    let dir = status_dir(status);
    if dir.is_empty() || slug.is_empty() {
        return None;
    }
    let content = fs::read_to_string(format!("{}/{}.md", dir, slug)).await.ok()?;
    Some(parse_front_matter(&content).0)
}

/// Shared save routine for create + update. Returns a redirect. `author` is the
/// creating writer, recorded for new posts only.
async fn save_post(db: &ForumDb, form: PostForm, author: &str) -> axum::response::Response {
    if form.title.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "Title is required").into_response();
    }
    let publish = form.action != "draft";
    let target_dir = if publish { POSTS_DIR } else { DRAFTS_DIR };
    let is_edit = !form.orig_slug.is_empty();

    // Existing front matter (edits only) — source of the stable creation time + image.
    let prior_fm = if is_edit {
        read_prior_fm(&form.orig_status, &form.orig_slug).await
    } else {
        None
    };

    // Filename + permalink and the post's creation timestamp.
    //   New post  → short "<title-slug>" (de-duplicated); date = now (UTC, RFC 3339)
    //               kept in front matter, not the filename, so URLs stay clean.
    //   Edit      → keep the original slug (stable permalink) and original creation time;
    //               only re-check uniqueness when the post moves to a different directory
    //               (draft↔published), where another file might already own that slug.
    let (slug, date) = if is_edit {
        let date = prior_fm
            .as_ref()
            .and_then(|fm| fm.date.clone())
            .unwrap_or_else(now_iso);
        let slug = if status_dir(&form.orig_status) != target_dir {
            ensure_unique_slug(&form.orig_slug, &[target_dir]).await
        } else {
            form.orig_slug.clone()
        };
        (slug, date)
    } else {
        let created = now_iso();
        // Unique across drafts and posts so authorship (keyed by slug) can't collide.
        let slug = ensure_unique_slug(&slugify(&form.title), &[POSTS_DIR, DRAFTS_DIR]).await;
        (slug, created)
    };

    // Resolve featured image: new upload wins, else keep the prior front-matter image.
    let image = if let (Some(bytes), Some(fname)) = (&form.image_bytes, &form.image_filename) {
        save_image(&slug, bytes, fname).await
    } else {
        prior_fm.as_ref().and_then(|fm| fm.image.clone())
    };

    if fs::create_dir_all(target_dir).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create directory").into_response();
    }
    let md = build_markdown(&form.title, &date, image.as_deref(), &form.body);
    let new_path = format!("{}/{}.md", target_dir, slug);
    if fs::write(&new_path, md).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write post").into_response();
    }

    if is_edit {
        move_post_author(db, &form.orig_slug, &slug).await;
    } else {
        set_post_author(db, &slug, author).await;
    }

    // Remove the old file if the location (status and/or slug) changed.
    if is_edit {
        let old_dir = status_dir(&form.orig_status);
        if !old_dir.is_empty() {
            let old_path = format!("{}/{}.md", old_dir, form.orig_slug);
            if old_path != new_path {
                let _ = fs::remove_file(&old_path).await;
            }
        }
    }

    if publish {
        Redirect::to(&format!("/blog/{}", slug)).into_response()
    } else {
        Redirect::to(&format!("/blog/edit/{}?saved=1", slug)).into_response()
    }
}

/// POST /blog/create — create a new post or draft (writer role required).
pub async fn create_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> impl IntoResponse {
    let v = match require_writer(&state, &headers, "/blog/new").await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match parse_post_form(multipart).await {
        Ok(mut form) => {
            // Creation never carries an original; ignore any stray fields.
            form.orig_slug.clear();
            form.orig_status.clear();
            save_post(&state.forum_db, form, v.email.as_deref().unwrap_or_default()).await
        }
        Err(e) => {
            eprintln!("create_post: {}", e);
            (StatusCode::BAD_REQUEST, "Invalid form submission").into_response()
        }
    }
}

/// POST /blog/update — update an existing post or draft (its author or an owner).
pub async fn update_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> impl IntoResponse {
    let v = match require_writer(&state, &headers, "/blog").await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match parse_post_form(multipart).await {
        Ok(mut form) => {
            form.orig_slug = sanitize_filename::sanitize(&form.orig_slug);
            if form.orig_slug.is_empty()
                || !v.can_edit(post_author(&state.forum_db, &form.orig_slug).await.as_deref())
            {
                return (StatusCode::FORBIDDEN, "You can only edit your own posts").into_response();
            }
            save_post(&state.forum_db, form, v.email.as_deref().unwrap_or_default()).await
        }
        Err(e) => {
            eprintln!("update_post: {}", e);
            (StatusCode::BAD_REQUEST, "Invalid form submission").into_response()
        }
    }
}

/// POST /blog/upload-image — store one image, return its URL as JSON (for the WYSIWYG
/// editor's inline-image hook). Writer role required.
pub async fn upload_image(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    _q: Query<std::collections::HashMap<String, String>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if let Err(r) = require_writer(&state, &headers, "/blog/new").await {
        return r;
    }
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.file_name().is_some() {
            let original = field.file_name().unwrap_or("image").to_string();
            let data = match field.bytes().await {
                Ok(d) if !d.is_empty() => d,
                _ => continue,
            };
            if let Some(url) = save_image("inline", &data, &original).await {
                return Json(json!({ "url": url })).into_response();
            }
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save image").into_response();
        }
    }
    (StatusCode::BAD_REQUEST, "No image uploaded").into_response()
}
