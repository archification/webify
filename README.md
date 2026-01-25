# Webifying Webififications of Webified Webbing

## Introduction
Welcome to the Webify project!

This CLI application is configurable web server designed to allow the hosting of an entire online community.

## Features
* **Multi-Domain Support:** Host different sets of routes and files for different hostnames using the sites configuration.
* **IP Whitelisting:** Restrict access to specific domains or the entire server based on client IP addresses.
* **Markdown Blogging:** Automatically renders .md files into clean, readable blog posts with built-in syntax highlighting (Solarized theme).
* **Interactive Slideshow:**
  * Markdown-based slide creation.
  * On-the-fly UI controls for autoplay and timer intervals.
  * Persistence of user settings via localStorage.
  * Keyboard navigation support.
* **Upload & Storage Management:**
  * Configure maximum file size for individual uploads.
  * Enforce a total storage limit for the uploads/ directory to prevent disk exhaustion.
* **Dynamic Media Galleries:** Supports rendering directories of images, videos, audio, and PDFs with optional "random" or "alphanumeric" sorting.
* **Live Thumbnails:** Efficiently generates and serves 150x150 thumbnails for image galleries on demand.
* **Diskless Architecture:** Files (images, GIFs, videos) are never saved to the server's disk. They are encoded as Base64 Data URIs on the client side and broadcast directly to connected clients via WebSockets.
* **Secure & Ephemeral:** When a room is empty, it dissolves completely. No logs, no files, no traces left behind.
* **Robust Media Support:** * Supports high-res Images, animated GIFs, and short Videos (up to 50MB).
    * Automatic link parsing for clickable URLs.
* **XSS Protection:** Custom-built text sanitization ensures user input is treated as text-only, preventing script injection while allowing safe media embedding.
* **Role-Based Interaction:** Distinct views for "Controllers" (sending signals) and "Doers" (receiving visual cues).

## Getting Started
1. Run the Binary: Place webify in an empty folder and execute it from your terminal.
2. Generate Environment: If no config.toml is found, the program will offer to create a complete example project structure, including sample HTML templates, Markdown posts, and directories.
3. Configure: Open the generated config.toml to customize your routes, ports, and security settings.
4. Restart: Run the program again to launch the server with your new configuration.

### Images
Program not detecting a present config file.  
![screenshot](https://github.com/archification/webify/blob/main/images/noconfig.png)

Files after generating them.  
![screenshot](https://github.com/archification/webify/blob/main/images/files.png)

Contents of the generated config file.  
![screenshot](https://github.com/archification/webify/blob/main/images/config.png)

Output of the running program.  
![screenshot](https://github.com/archification/webify/blob/main/images/running.png)

## Technical Notes
* Dual Protocol: HTTP and HTTPS are served on the same port when SSL is enabled, with automatic u pgrades from HTTP to HTTPS.
* Scope Overrides: Setting `scope` to `localhost` or `public` will override the manual ip field with 127.0.0.1 or 0.0.0.0 respectively.
* Help: Run `webify -h` or `webify --help` at any time to see a detailed breakdown of configuration options.

## Build Process
* We use cross for compiling staticically linked binary with musl for linux. This requires docker to be running.
* We use xwin for compiling with msvc for windows.
* See the build.sh script for more information.
