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
        out.push(PostMeta { slug, title, image: fm.image, date: fm.date, date_display });
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

/// True if the request carries a valid guard session whose email is permitted by the
/// same auth guard that protects the authoring routes (`/blog/new`).
pub async fn viewer_can_edit(state: &Arc<AppState>, headers: &HeaderMap) -> bool {
    let host = host_from_headers(headers);
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = match auth_guard::extract_cookie_value(cookie_header, auth_guard::GUARD_COOKIE) {
        Some(t) => t,
        None => return false,
    };
    let email = match auth_guard::validate_session(&state.forum_db, &token).await {
        Some(e) => e,
        None => return false,
    };
    match auth_guard::find_guard(&state.config.auth_guards, &host, "/blog/new") {
        Some(guard) => auth_guard::email_allowed(guard, &email),
        None => false,
    }
}

// ─── Read handlers ───────────────────────────────────────────────────────────

/// GET /blog — public index. Lists published posts; also lists drafts for writers.
pub async fn blog_index(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let can_edit = viewer_can_edit(&state, &headers).await;
    let posts = list_dir(POSTS_DIR).await;
    let drafts = if can_edit { list_dir(DRAFTS_DIR).await } else { Vec::new() };

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

/// GET /blog/new — empty authoring form (auth-guarded in the router fallback).
pub async fn new_post_form(State(state): State<Arc<AppState>>) -> impl IntoResponse {
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
    Query(q): Query<std::collections::HashMap<String, String>>,
    AxumPath(slug): AxumPath<String>,
) -> impl IntoResponse {
    let slug = sanitize_filename::sanitize(&slug);
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

/// Find a slug not already used by a different file in `dir` (skips `exclude`).
async fn ensure_unique_slug(base: &str, dir: &str, exclude: &str) -> String {
    let mut slug = base.to_string();
    let mut n = 2;
    while slug != exclude
        && fs::try_exists(format!("{}/{}.md", dir, slug)).await.unwrap_or(false)
    {
        slug = format!("{}-{}", base, n);
        n += 1;
    }
    slug
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

/// Shared save routine for create + update. Returns a redirect.
async fn save_post(form: PostForm) -> axum::response::Response {
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
            ensure_unique_slug(&form.orig_slug, target_dir, "").await
        } else {
            form.orig_slug.clone()
        };
        (slug, date)
    } else {
        let created = now_iso();
        let slug = ensure_unique_slug(&slugify(&form.title), target_dir, "").await;
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

/// POST /blog/create — create a new post or draft (auth-guarded).
pub async fn create_post(multipart: Multipart) -> impl IntoResponse {
    match parse_post_form(multipart).await {
        Ok(mut form) => {
            // Creation never carries an original; ignore any stray fields.
            form.orig_slug.clear();
            form.orig_status.clear();
            save_post(form).await
        }
        Err(e) => {
            eprintln!("create_post: {}", e);
            (StatusCode::BAD_REQUEST, "Invalid form submission").into_response()
        }
    }
}

/// POST /blog/update — update an existing post or draft (auth-guarded).
pub async fn update_post(multipart: Multipart) -> impl IntoResponse {
    match parse_post_form(multipart).await {
        Ok(mut form) => {
            form.orig_slug = sanitize_filename::sanitize(&form.orig_slug);
            save_post(form).await
        }
        Err(e) => {
            eprintln!("update_post: {}", e);
            (StatusCode::BAD_REQUEST, "Invalid form submission").into_response()
        }
    }
}

/// POST /blog/upload-image — store one image, return its URL as JSON (for the WYSIWYG
/// editor's inline-image hook). Auth-guarded.
pub async fn upload_image(
    _q: Query<std::collections::HashMap<String, String>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
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
