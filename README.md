# Webifying Webififications of Webified Webbing

## Introduction
Welcome to the Webify project!
Have you ever wanted to start a web server at a moment's notice so that your friends could download files from your computer or you could view files on one machine from any device on your local wifi?
This program allows you to do just that. After having a brief look at a config file and running a standalone program from command line, you'll be able to view and share any content you've made.

This CLI application is configurable web server developed in Rust using the `axum` crate.
The config.toml file allows us to determine paths to web routes and local files, IP address and port, and whether things like SSL are enabled.

## Features
- Configuration via a toml config file including routes, paths, ip, and port.
- SSL support.
- HTMX todo page.
- Server exists as a single binary for portability.
- Can open the default system browser to the configured home page on launch.

## Getting Started
- Place the program file inside an empty folder.
- Run the file from either a windows cmd or linux terminal.
- If the program asks to generate default files, type the letter y and press enter.
- View and modify the config file in notepad or any text editor.
- Run the program again to use this config file.

### Notes
- Running `webify -h` or `webify --help` will show help output with further information.
- The ip field is only used if scope is lan. A scope of localhost overrides the ip field with 127.0.0.1 and a scope of public overrides the ip field with 0.0.0.0.
- Dual protocol is built-in. This means http and https exist on the same port when SSL is enabled.
- Ensure that if todo is enabled, it's assigned a different port than the main server.

Program not detecting a present config file. 
![screenshot](https://github.com/archification/webify/blob/main/images/noconfig.png)

Files after generating them. 
![screenshot](https://github.com/archification/webify/blob/main/images/files.png)

Contents of the generated config file. 
![screenshot](https://github.com/archification/webify/blob/main/images/config.png)

Output of the running program. 
![screenshot](https://github.com/archification/webify/blob/main/images/running.png)

The todo page. 
![screenshot](https://github.com/archification/webify/blob/main/images/todo.png)

## Background
This project started from a desire to make media and other files accessible via browser from other devices on the lan.
The became larger as I learned more and this process will likely continue over time into the future.
