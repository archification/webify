# Webifying Webififications of Webified Webbing

## Introduction
Welcome to the Webify project!

This CLI application is configurable web server developed in Rust using the `axum` crate.
It is designed to be a single-binary solution for serving media, documents, and interactive content across a local network or the web.

## Features
* Multi-Domain Support: Host different sets of routes and files for different hostnames using the sites configuration.
* IP Whitelisting: Restrict access to specific domains or the entire server based on client IP addresses.
* Markdown Blogging: Automatically renders .md files into clean, readable blog posts with built-in syntax highlighting (Solarized theme).
* Interactive Slideshow:
  * Markdown-based slide creation.
  * On-the-fly UI controls for autoplay and timer intervals.
  * Persistence of user settings via localStorage.
  * Keyboard navigation support.
* Upload & Storage Management:
  * Configure maximum file size for individual uploads.
  * Enforce a total storage limit for the uploads/ directory to prevent disk exhaustion.
* Dynamic Media Galleries: Supports rendering directories of images, videos, audio, and PDFs with optional "random" or "alphanumeric" sorting.
* Live Thumbnails: Efficiently generates and serves 150x150 thumbnails for image galleries on demand.

## Getting Started
1. Run the Binary: Place webify in an empty folder and execute it from your terminal.
2. Generate Environment: If no config.toml is found, the program will offer to create a complete example project structure, including sample HTML templates, Markdown posts, and directories.
3. Configure: Open the generated config.toml to customize your routes, ports, and security settings.
4. Restart: Run the program again to launch the server with your new configuration.

## Build Process
The project nwo utilizes cross for multi-platform compilation and upx for binary compression to ensure maximum portability and minimal file size.
```sh
cross build --target x86_64-unknown-linux-musl --release
upx --best --lzma target/x86_64-unknown-linux-musl/release/webify
```

## Technical Notes
* Dual Protocol: HTTP and HTTPS are served on the same port when SSL is enabled, with automatic u pgrades from HTTP to HTTPS.
* Scope Overrides: Setting `scope` to `localhost` or `public` will override the manual ip field with 127.0.0.1 or 0.0.0.0 respectively.
* Help: Run `webify -h` or `webify --help` at any time to see a detailed breakdown of configuration options.

### Images
Program not detecting a present config file.  
![screenshot](https://github.com/archification/webify/blob/main/images/noconfig.png)

Files after generating them.  
![screenshot](https://github.com/archification/webify/blob/main/images/files.png)

Contents of the generated config file.  
![screenshot](https://github.com/archification/webify/blob/main/images/config.png)

Output of the running program.  
![screenshot](https://github.com/archification/webify/blob/main/images/running.png)

## Background
This project started from a desire to make media and other files accessible via browser from other devices on the LAN. It has since grown into a moduler service capable of handling blogging, presentations, and secure file sharing.
