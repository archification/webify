use std::fs;
use std::path::PathBuf;
use axum::{
    extract::{DefaultBodyLimit, Query},
    routing::{
        get, post, get_service
    },
    http::StatusCode, response::{
        Html, IntoResponse
    },
    Router
};
use tower_http::services::{ServeDir, ServeFile};
use serde::Deserialize;
use pulldown_cmark::{Parser, Options, html};
use crate::config::Config;
use crate::media::{render_html, render_html_with_media};
use crate::upload::upload;
use crate::limits::parse_upload_limit;
use solarized::{
    print_fancy,
    VIOLET, CYAN, RED, ORANGE,
    BOLD,
    PrintMode::NewLine,
};

#[derive(Debug, Deserialize)]
struct SlideQuery {
    current: Option<usize>,
}

async fn read_markdown_slides(slides_dir: &str) -> Result<Vec<(String, PathBuf)>, std::io::Error> {
    let mut dir = tokio::fs::read_dir(slides_dir).await?;
    let mut entries = Vec::new();
    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("md") {
                    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                        let slide_name = filename.trim_end_matches(".md").to_string();
                        entries.push((slide_name, path));
                    }
                }
            }
        }
    }
    entries.sort_by(|(a, _), (b, _)| {
        a.parse::<usize>().unwrap_or(0).cmp(&b.parse::<usize>().unwrap_or(0))
    });
    Ok(entries)
}

async fn handle_slideshow(
    Query(query): Query<SlideQuery>,
    slides_dir: &str,
    autoplay: bool,
    timer_seconds: u64,
) -> Html<String> {
    let slides = match read_markdown_slides(slides_dir).await {
        Ok(s) => s,
        Err(e) => return Html(format!("Error reading slides directory: {}", &e)),
    };
    if slides.is_empty() {
        return Html("No slides found".to_string());
    }
    let current = query.current.unwrap_or(0);
    let total = slides.len();
    let current_index = current % total;
    let (_, current_slide_path) = &slides[current_index];
    let markdown_content = match tokio::fs::read_to_string(current_slide_path).await {
        Ok(content) => content,
        Err(e) => return Html(format!("Error reading slide file: {}", &e)),
    };
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    let parser = Parser::new_ext(&markdown_content, options);
    let mut html_content = String::new();
    html::push_html(&mut html_content, parser);
    let slide_number = current_index + 1;
    let html = format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Slideshow</title>
    <!-- Add highlight.js for syntax highlighting -->
    <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/base16/solarized-dark.min.css">
    <script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>
    <script>hljs.highlightAll();</script>
    <style>
        :root {{
            --base03: #002b36;
            --base02: #073642;
            --base01: #586e75;
            --base00: #657b83;
            --base0: #839496;
            --base1: #93a1a1;
            --base2: #eee8d5;
            --base3: #fdf6e3;
            --yellow: #b58900;
            --orange: #cb4b16;
            --red: #dc322f;
            --magenta: #d33682;
            --violet: #6c71c4;
            --blue: #268bd2;
            --cyan: #2aa198;
            --green: #859900;
        }}
        body {{
            background-color: var(--base03);
            color: var(--base0);
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
            line-height: 1.6;
            margin: 0;
            padding: 20px;
        }}
        .slide-container {{
            max-width: 900px;
            margin: 0 auto;
            background-color: var(--base02);
            border-radius: 8px;
            padding: 30px;
            box-shadow: 0 4px 20px rgba(0,0,0,0.5);
        }}
        h1, h2, h3, h4 {{
            color: var(--yellow);
            margin-top: 0;
        }}
        a {{
            color: var(--blue);
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
        pre {{
            background-color: var(--base03);
            border: 1px solid var(--base01);
            border-radius: 4px;
            padding: 15px;
            overflow: auto;
        }}
        blockquote {{
            border-left: 4px solid var(--cyan);
            padding-left: 15px;
            margin-left: 0;
            color: var(--base1);
        }}
        .navigation {{
            display: flex;
            justify-content: center;
            gap: 15px;
            margin-top: 30px;
        }}
        .navigation button {{
            background: var(--yellow);
            color: var(--base03);
            border: none;
            padding: 10px 20px;
            border-radius: 4px;
            cursor: pointer;
            font-weight: bold;
            font-size: 1rem;
            transition: background 0.2s;
        }}
        .navigation button:hover {{
            background: var(--orange);
        }}
        .slide-counter {{
            text-align: center;
            margin-top: 10px;
            color: var(--base1);
            font-size: 1.1rem;
        }}
        .slide-content {{
            min-height: 400px;
        }}
        .slide-footer {{
            margin-top: 20px;
            text-align: center;
            color: var(--base01);
            font-size: 0.9rem;
        }}
        /* Improve code block appearance */
        .hljs {{
            background: var(--base03) !important;
            padding: 1em !important;
            border-radius: 4px;
        }}
    </style>
    <style>
        /* Update controls container */
        .controls-container {{
            position: fixed;
            bottom: 20px;
            right: 20px;
            z-index: 90;
            transition: transform 0.3s ease-in-out;
            transform: translateY(calc(100% + 20px));
        }}
        .controls-container.visible {{
            transform: translateY(0);
        }}
        .controls {{
            background: rgba(0, 0, 0, 0.7);
            padding: 10px 20px;
            border-radius: 20px;
            display: flex;
            gap: 15px;
            align-items: center;
            z-index: 100;
            backdrop-filter: blur(5px);
            box-shadow: 0 4px 10px rgba(0,0,0,0.5);
        }}
        .controls:hover {{
            background: rgba(0, 0, 0, 0.9);
        }}
        .controls label {{
            display: flex;
            align-items: center;
            gap: 5px;
            color: #eee8d5;
        }}
        .controls input[type="number"] {{
            width: 50px;
            padding: 5px;
            background: #002b36;
            color: #93a1a1;
            border: 1px solid #586e75;
            border-radius: 4px;
        }}
        .controls button {{
            background: #b58900;
            color: #002b36;
            border: none;
            padding: 5px 10px;
            border-radius: 4px;
            cursor: pointer;
            font-weight: bold;
        }}
    </style>
    <script>
        // Store settings in localStorage
        function saveSettings() {{
            const autoplay = document.getElementById('autoplayCheckbox').checked;
            const timer = document.getElementById('timerInput').value;
            localStorage.setItem('slideshowAutoplay', autoplay);
            localStorage.setItem('slideshowTimer', timer);
            // Restart autoplay with new settings
            stopAutoplay();
            if (autoplay) {{
                startAutoplay(timer);
            }}
        }}
        // Load settings from localStorage or use defaults
        function loadSettings() {{
            const savedAutoplay = localStorage.getItem('slideshowAutoplay');
            const savedTimer = localStorage.getItem('slideshowTimer');
            if (savedAutoplay !== null) {{
                document.getElementById('autoplayCheckbox').checked = savedAutoplay === 'true';
            }} else {{
                document.getElementById('autoplayCheckbox').checked = {default_autoplay};
            }}
            if (savedTimer !== null) {{
                document.getElementById('timerInput').value = savedTimer;
            }} else {{
                document.getElementById('timerInput').value = {default_timer};
            }}
        }}
        // MODIFIED MOUSE TRACKING FOR BOTTOM RIGHT CORNER
        document.addEventListener('mousemove', (e) => {{
            mouseY = e.clientY;
            mouseX = e.clientX;
            const windowHeight = window.innerHeight;
            const windowWidth = window.innerWidth;
            // Show controls when mouse is near bottom right corner
            if ((windowHeight - mouseY < 50) && (windowWidth - mouseX < 100)) {{
                showControls();
                clearTimeout(hideTimeout);
            }}
        }});
    </script>
</head>
<body>
    <div class="slide-container">
        <div class="slide-content">
            {html_content}
        </div>
        <div class="navigation">
            <button onclick="navigate(-1)">← Previous</button>
            <button onclick="navigate(1)">Next →</button>
        </div>
        <div class="slide-counter">Slide {slide_number}/{total} | Press ← → to navigate</div>
        <div class="slide-footer">
            Press ← → keys to navigate between slides
        </div>
    </div>
    <!-- Combined sensor and controls area -->
    <div class="controls-container">
        <div class="controls">
            <label>
                <input type="checkbox" id="autoplayCheckbox" {autoplay_checked}> Autoplay
            </label>
            <label>
                Timer (s): <input type="number" id="timerInput" min="1" max="60" value="{timer_seconds}">
            </label>
            <button onclick="saveSettings()">Apply</button>
        </div>
    </div>
    <script>
        // Initialize syntax highlighting
        hljs.highlightAll();
        // Get controls container
        const controlsContainer = document.querySelector('.controls-container');
        // Handle keyboard navigation
        document.addEventListener('keydown', function(event) {{
            if (event.key === 'ArrowLeft') {{
                navigate(-1);
                stopAutoplay();
            }} else if (event.key === 'ArrowRight') {{
                navigate(1);
                stopAutoplay();
            }}
        }});
        function navigate(direction) {{
            const newIndex = ({current_index} + direction + {total}) % {total};
            window.location.href = `?current=${{newIndex}}`;
        }}
        // Autoplay functionality
        let autoplayInterval;
        function startAutoplay(timer) {{
            const timerMs = timer * 1000;
            autoplayInterval = setInterval(() => navigate(1), timerMs);
        }}
        function stopAutoplay() {{
            clearInterval(autoplayInterval);
        }}
        // Initialize settings and autoplay
        loadSettings();
        // Start autoplay if enabled
        if (document.getElementById('autoplayCheckbox').checked) {{
            startAutoplay(document.getElementById('timerInput').value);
        }}
        // Controls visibility functions
        function showControls() {{
            controlsContainer.classList.add('visible');
        }}
        function hideControls() {{
            controlsContainer.classList.remove('visible');
        }}
        // Mouse tracking to show/hide controls
        let mouseY = 0;
        let hideTimeout;
        document.addEventListener('mousemove', (e) => {{
            mouseY = e.clientY;
            const windowHeight = window.innerHeight;
            // Show controls when mouse is near bottom
            if (windowHeight - mouseY < 50) {{
                showControls();
                clearTimeout(hideTimeout);
            }}
        }});
        // Hide controls when mouse moves away from bottom
        document.addEventListener('mouseleave', () => {{
            hideTimeout = setTimeout(hideControls, 500);
        }});
        // Keep controls visible when interacting with them
        controlsContainer.addEventListener('mouseenter', () => {{
            clearTimeout(hideTimeout);
        }});
        controlsContainer.addEventListener('mouseleave', () => {{
            hideTimeout = setTimeout(hideControls, 500);
        }});
        // Pause autoplay on user interaction
        document.querySelectorAll('.navigation button').forEach(button => {{
            button.addEventListener('click', () => {{
                stopAutoplay();
                showControls();
            }});
        }});
    </script>
</body>
</html>"#,
        html_content = html_content,
        slide_number = slide_number,
        total = total,
        autoplay_checked = if autoplay { "checked" } else { "" },
        default_autoplay = autoplay,
        default_timer = timer_seconds,
        timer_seconds = timer_seconds,
        current_index = current_index,
    );
    Html(html)
}

async fn not_found() -> impl IntoResponse {
    let file_path = "static/error.html";
    let custom_404_html = fs::read_to_string(file_path).unwrap_or_else(|_| {
    String::from(r#"
<!doctype html>
<html lang="en-US">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
    <title>guacamole</title>
    <link rel="stylesheet" type="text/css" href="https://thomasf.github.io/solarized-css/solarized-dark.min.css"></link>
    <meta name="viewport" content="width=device-width, initial-scale=1">
</head>
<body>
    <h1>ERROR</h1>
    <p>You shouldn't be here. Please go away.</p>
</body>
</html>
"#)
    });
    (StatusCode::NOT_FOUND, Html(custom_404_html))
}

fn routes_static() -> Router {
    Router::new().nest_service("/static", get_service(ServeDir::new("static")))
}

fn routes_uploads() -> Router {
    Router::new().nest_service("/uploads", get_service(ServeDir::new("uploads")))
}

pub async fn app(config: &Config) -> Router {
    let mut router = Router::new()
        .merge(routes_static())
        .merge(routes_uploads())
        .route("/favicon.ico", get_service(ServeFile::new("static/favicon.ico")))
        .nest_service("/styles", ServeDir::new("styles"))
        .nest_service("/scripts", ServeDir::new("scripts"))
        .nest_service("/images", ServeDir::new("images"))
        .fallback(get(not_found));
    for (path, settings) in &config.routes {
        match settings.as_slice() {
            [settings_type, slides_dir] if settings_type == "slideshow" => {
                let slides_dir_clone = slides_dir.clone();
                let autoplay = config.slideshow_autoplay;
                let timer = config.slideshow_timer;
                router = router.route(
                    path,
                    get(move |query: Query<SlideQuery>| {
                        let dir = slides_dir_clone.clone();
                        async move {
                            handle_slideshow(query, &dir, autoplay, timer).await
                        }
                    }),
                );
            }
            [file_path, media_dir] => {
                let media_route = path.trim_start_matches('/');
                let file_clone = file_path.clone();
                let media_dir_clone = media_dir.clone();
                let media_route_clone = media_route.to_string();
                router = router.route(path, get(move || {
                    let file = file_clone.clone();
                    let media = media_dir_clone.clone();
                    let route = media_route_clone.clone();
                    async move {
                        render_html_with_media(&file, &media, &route).await
                    }
                }));
                let serve_dir = ServeDir::new(media_dir);
                router = router
                    .nest_service(&format!("/static/{media_route}"), serve_dir);
            }
            [file_path] => {
                let file_clone = file_path.clone();
                router = router.route(path, get(move || {
                    async move {
                        render_html(&file_clone).await
                    }
                }));
            }
            _ => {}
        }
    }
    let something = config.upload_storage_limit;
    match parse_upload_limit(&config.upload_size_limit).await {
        Ok(num) => {
            router = router.route(
                "/upload",
                post(move |multipart| upload(multipart, something))
                .layer(DefaultBodyLimit::max(num))
            );
        },
        Err("disabled") => {
            router = router.route(
                "/upload",
                post(move |multipart| upload(multipart, something))
                .layer(DefaultBodyLimit::disable())
            );
        },
        _ => {
            print_fancy(&[
                ("Error", RED, vec![BOLD]),
                (": ", CYAN, vec![]),
                ("config.upload_size_limit", VIOLET, vec![]),
                (" is ", CYAN, vec![]),
                ("null", ORANGE, vec![]),
                (": ", CYAN, vec![]),
                ("Defaulting to ", CYAN, vec![]),
                ("2 * 1000 * 1000 * 1000 || 2GB", VIOLET, vec![]),
            ], NewLine);
            let default_limit = 2 * 1000 * 1000 * 1000;
            router = router.route(
                "/upload",
                post(move |multipart| upload(multipart, something))
                .layer(DefaultBodyLimit::max(default_limit))
            );
        }
    }
    router
}
