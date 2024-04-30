# Webifying Webififications of Webified Webbing

## Introduction
Welcome to the Webify project! This application, developed in Rust, uses `axum` to create a dynamic web server.
The server reads from a configuration file to set up web mount points, paths to HTML and media files,
as well as the IP address and port number for the server.

## Features
- Dynamic configuration via a toml config file including mount points, local file paths, ip, and port.
- SSL support can be enabled in the config file.
- HTMX todo page can be enabled in the config file.
- Easy setup and deployment.

## Getting Started
- Place the exe file alone in a folder.
- Run the exe file.
- It will ask if you'd like to generate files. Type y and press enter to do so.
- Ensure that files were generated in the folder.
- Run the exe file again to host example environment locally.
- Modify settings in the config file as you see fit and then restart the program.

### Prerequisites
- Web browser.
- Basic knowledge of HTML and file paths.
- Ability to run an exe file in a cli environment.

### Installation
Download the exe for your operating system of choice and place it in a folder.
Running the program will generate an example file structure including a qrcode.png hosted at /stuff which links to http://localhost:12345/ as the ip and port in the default generated config is 127.0.0.1:12345
Running `webify -h` or `webify --help` will show help output with further information.
