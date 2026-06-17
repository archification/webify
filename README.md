# Webify

A configurable, single-binary web server designed to host an entire online community. Supports multi-domain routing, forums, live streaming, blogs, media galleries, real-time interaction rooms, file uploads, and more -- all from one `config.toml`.

## Features

### Core Server
* **Multi-Domain Routing:** Host different sites on different hostnames from a single instance using the `[routes."hostname"]` config.
* **SSL/TLS:** HTTPS via rustls with separate HTTP and HTTPS listeners on configurable ports.
* **Automatic Let's Encrypt:** Built-in ACME HTTP-01 challenge support (`acme_enabled`) for automatic certificate issuance and renewal. Configurable staging/production, renewal threshold, and account persistence.
* **HTTP Mode Control:** Choose how the plain-HTTP listener behaves when SSL is enabled: `"serve"` (full app), `"redirect"` (308 to HTTPS), or `"https_only"` (only answer ACME challenges).
* **IP Whitelisting:** Restrict access to specific domains or the entire server by client IP address.
* **Scope Shortcuts:** Setting `scope` to `localhost`, `lan`, or `public` automatically binds to `127.0.0.1`, your LAN IP, or `[::]` respectively.

### Content & Media
* **Tera Templating:** All HTML pages are rendered through the Tera template engine with access to config variables like `port` and `domain`.
* **Markdown Blogging:** Full blog system with front-matter support (title, date, image), a post index, create/edit forms, image uploads, and syntax highlighting. Blog editing is restricted via auth guard.
* **Interactive Slideshows:** Markdown-based slide creation with autoplay, configurable timer intervals, keyboard navigation, and localStorage persistence.
* **Dynamic Media Galleries:** Render directories of images, videos, audio, and PDFs with optional `"random"` or `"alphanumeric"` sorting.
* **Live Thumbnails:** On-demand 150x150 thumbnail generation for image galleries.
* **Static File Serving:** Serve any directory as static files using the `"static"` route mode (useful for wikis, documentation, etc.).
* **Live Log Viewer:** Watch a file in real-time via HTMX polling with the `"live"` route mode.
* **PDF Viewer:** Built-in PDF rendering and document gallery support.

### Forum
* **Full Forum System:** Category-based threaded forum with registration, email verification, and role-based permissions (Member, Admin, Owner).
* **Google OAuth:** Users can register and log in via Google OAuth in addition to username/password.
* **Admin Panel:** Manage users (ban/unban, set roles), categories (create/delete), and posts (edit/delete/lock) from a web UI.
* **Reply Restrictions:** Thread authors and admins can set minimum role requirements for replies.
* **SMTP Email Verification:** Configurable SMTP integration for account verification emails.
* **Separate Config:** Forum categories and admin emails are configured in `forum.toml`.

### Live Streaming (WebRTC)
* **WHIP/WHEP Streaming:** Browser-based live streaming using standard WebRTC WHIP (ingest) and WHEP (playback) protocols.
* **Stream Key Auth:** Admins get auto-generated stream keys stored in the database. Only admin-role users can broadcast.
* **Public Viewing:** Anyone can watch an active stream at `/watch` with automatic live status polling.
* **NAT Traversal:** Configurable `public_ip` for WebRTC ICE candidate handling behind NAT.

### Interaction Rooms
* **WebSocket Rooms:** Create and join real-time interaction rooms with role-based participation (Controllers and Doers).
* **Permanent Rooms:** Pre-configure rooms in `config.toml` that persist across restarts with optional passwords.
* **Diskless Media Sharing:** Images, GIFs, and videos are base64-encoded client-side and broadcast via WebSocket -- nothing is saved to disk.
* **Ephemeral by Default:** Non-permanent rooms dissolve when empty. No logs, no files, no traces.
* **XSS Protection:** Custom text sanitization prevents script injection while allowing safe media embedding.
* **Chat Commands:** Extensible command system for interaction rooms.

### Access Control
* **Auth Guard (Google OAuth):** Protect any URL path with Google OAuth email verification. Restrict by specific email addresses or entire email domains. Configurable per-hostname.
* **File Gate:** Protect paths with SHA-256 key file authentication. Users upload a file whose hash must match the configured digest. Manageable from the admin dashboard.
* **Admin Dashboard:** Web UI for managing auth guard access rules, dashboard editors, and file guards. Owner emails are set in config; editors can be added/revoked from the UI.
* **Database-Backed Rules:** Access rules and file guards can be managed at runtime through the admin dashboard in addition to config-file definitions.

### Upload & Storage
* **Configurable Upload Limits:** Set max file size per upload (or `"disabled"` for unlimited).
* **Storage Quota:** Enforce a total storage limit for the `uploads/` directory to prevent disk exhaustion.

### PHP Support
* **PHP-FPM Proxy:** Route paths to a PHP-FPM backend via FastCGI using the `"php"` route mode.

## Getting Started

1. **Run the binary:** Place `webify` in an empty folder and execute it from your terminal.
2. **Generate environment:** If no `config.toml` is found, the program will offer to create a complete example project structure, including sample HTML templates, markdown posts, forum templates, streaming pages, and directories.
3. **Configure:** Edit the generated `config.toml` to customize your routes, ports, domains, and security settings. Edit `forum.toml` to configure forum categories and admin emails.
4. **Restart:** Run the program again to launch the server with your new configuration.

### Route Types

Routes are configured in `config.toml` under `[routes]`. Each route maps a URL path to a handler:

| Format | Mode | Description |
|--------|------|-------------|
| `["/template.html"]` | Template | Render a Tera HTML template |
| `["/template.html", "/media/dir"]` | Gallery | Media gallery with optional sort (`"random"` or `"alphanumeric"` as 3rd element) |
| `["slideshow", "/slides/dir"]` | Slideshow | Markdown-based slideshow from a directory of `.md` files |
| `["/template.html", "forum"]` | Forum | Mount the full forum system at this path |
| `["/dir", "static"]` | Static | Serve a directory as static files |
| `["/template.html", "/file/to/watch", "live"]` | Live | Live log/file viewer with HTMX polling |
| `["/doc/root", "fpm_addr", "php"]` | PHP | Proxy to PHP-FPM via FastCGI |

### Built-in Routes

These routes are registered automatically (unless overridden by a config route at the same path):

* `/blog` -- Blog index, individual posts at `/blog/{slug}`, create/edit forms
* `/live` -- Streaming dashboard (admin-only broadcast)
* `/watch` -- Stream viewer (public)
* `/interaction` -- Interaction room lobby
* `/upload` -- File upload endpoint
* `/thumbnail/{path}` -- On-demand thumbnail generation
* `/auth/*` -- Auth guard login/callback/logout endpoints
* `/.well-known/acme-challenge/{token}` -- ACME HTTP-01 challenge responder

### Images

See the [example config template](src/templates/config.toml) for a complete reference of all configuration options.

Program not detecting a present config file.
![screenshot](https://github.com/archification/webify/blob/main/images/noconfig.png)

Output of the running program.
![screenshot](https://github.com/archification/webify/blob/main/images/running.png)

## Technical Notes

* **Two-Listener Model:** When SSL is enabled, HTTP and HTTPS run on separate ports (`port` and `ssl_port`). In production, use iptables to route `:80 -> port` and `:443 -> ssl_port`.
* **Scope Overrides:** Setting `scope` to `localhost` or `public` will override the `ip` field with `127.0.0.1` or `[::]` respectively. `lan` uses the configured `ip`.
* **SQLite Database:** Forum users, posts, replies, stream keys, auth sessions, access rules, and file guards are stored in `forum.db` (auto-created with WAL mode).
* **Help:** Run `webify -h` or `webify --help` at any time to see a breakdown of configuration options.

## Build Process

* We use `cross` for compiling a statically linked binary with musl for linux. This requires Docker to be running.
* We use `xwin` for compiling with MSVC for Windows.
* See the `build.sh` script for more information.
