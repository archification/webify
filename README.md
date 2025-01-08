# Webifying Webififications of Webified Webbing

## Introduction
Welcome to the Webify project! Have you ever wanted to start a web server at a moment's notice so that your friends could download files from your computer? This program allows you to do just that. After having a brief look at a config file and running a standalone program from command line, you'll be able to view and share any content you've made.

This CLI application is configurable web server developed in Rust using the `axum` crate.
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
- Running `webify -h` or `webify --help` will show help output with further information.
- Plans for adding support to have the server binary generate source and compile a new binary with the user's config.toml and files embedded to be generated instead of the default config and files.

![screenshot](https://github.com/archification/webify/blob/main/images/noconfig.png)
![screenshot](https://github.com/archification/webify/blob/main/images/running.png)

## Background
This project started because I wanted an easy way to share media and other files on a web page to be viewed on my other devices on the lan but eventually became my main project.
One day a friend asked if he could send me a file that was too large to upload through discord so I added file uploading.
Another day I made a color highlighting crate out of boredom and thought I'd use it in this project.
After adding feature after feature over time, eventually we get here.
This project is where I learned most of what I know about Rust and really most of what I know about programming in general.
It's become a sort of default thing I contribute to whenever I feel like coding.
