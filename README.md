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

### Installation
- Download the exe for your operating system of choice and place it in a folder.
- The program will ask to generate an example file structure including a config file.
- Edit the config as you please.
- Running `webify -h` or `webify --help` will show help output with further information.
