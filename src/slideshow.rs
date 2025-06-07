use std::path::PathBuf;
use axum::{
    extract::Query,
    response::Html
};
use serde::Deserialize;
use pulldown_cmark::{Parser, Options, html};

#[derive(Debug, Deserialize)]
pub struct SlideQuery {
    current: Option<usize>,
}

async fn read_markdown_slides(slides_dir: &str) -> Result<Vec<(String, PathBuf)>, std::io::Error> {
    let mut dir = tokio::fs::read_dir(slides_dir).await?;
    let mut entries = Vec::new();
    while let Some(entry) = dir.next_entry().await? {
        let metadata = entry.metadata().await?;
        if !metadata.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("md") {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        let slide_name = filename.trim_end_matches(".md").to_string();
        entries.push((slide_name, path));
    }
    entries.sort_by(|(a, _), (b, _)| {
        a.parse::<usize>().unwrap_or(0).cmp(&b.parse::<usize>().unwrap_or(0))
    });
    Ok(entries)
}

pub async fn handle_slideshow(
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
