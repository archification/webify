# Webifying Webififications of Webified Webbing

## Introduction
Welcome to the Webify project!
This CLI application is developed in Rust using the `axum` crate makes up a configurable web server.
The config.toml file allows us to determine paths to web routes and local files, IP address and port, and whether things like SSL are enabled.

## Features
- Configuration via a toml config file including routes, paths, ip, and port.
- SSL support.
- HTMX todo page.
- Server exists as a single binary for portability.
- Can open the default system browser to the configured home page on launch.

## Getting Started
- Create a directory/folder and throw the exe in there.
- Run the exe file inside a cli.
- It will ask if you'd like to generate files. Type y and press enter.
- Make sure files were generated in the folder.
- Run the exe file again to start example environment with defautl config locally.
- Modify settings in the config file as you see fit and then restart the program.

### Notes
- Edit the config as you please.
- Running `webify -h` or `webify --help` will show help output with further information.
- Plans for adding support to have the server binary generate source and compile a new binary with the user's config.toml and files embedded to be generated instead of the default config and files.

![screenshot](https://github.com/archification/webify/blob/main/images/noconfig.png)
![screenshot](https://github.com/archification/webify/blob/main/images/running.png)
