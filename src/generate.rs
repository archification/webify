use crate::constants::*;

//use std::path::{Path, PathBuf};
use std::path::Path;
//use std::io::BufReader;
use std::env;
//use std::fs::{self, File};
use std::io;
use std::fs;
//use zip::ZipArchive;
use solarized::{
    print_fancy, clear,
    VIOLET, BLUE, CYAN, GREEN, YELLOW, ORANGE, RED,
    PrintMode::NewLine,
};

pub fn generate_files() {
    fn check_file_exists<P: AsRef<Path>>(path: P, filename: &str) -> bool {
        if path.as_ref().exists() {
            print_fancy(&[
                (filename, VIOLET, vec![]),
                (" already exists", YELLOW, vec![]),
            ], NewLine);
            true
        } else {
            false
        }
    }
    print_fancy(&[
        ("Failed to read configuration\n", ORANGE, vec![]),
        ("Example environment can be created in the current active directory.\n", CYAN, vec![]),
        ("Would you like to create an example environment?\n", CYAN, vec![]),
        ("(", VIOLET, vec![]), ("y", BLUE, vec![]),
        ("/", VIOLET, vec![]),
        ("n", RED, vec![]),
        (")", VIOLET, vec![]),
    ], NewLine);
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim().to_lowercase();
    if input == "y" || input == "yes" {
        clear();
        let config_path = Path::new("config.toml");
        if !check_file_exists(config_path, "config.toml") {
            match fs::write(config_path, EXAMPLE_CONFIG) {
                Ok(_) => {
                    print_fancy(&[
                        ("Example ", CYAN, vec![]),
                        ("config.toml", VIOLET, vec![]),
                        (" file has been ", CYAN, vec![]),
                        ("created.", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Failed to create example config.toml file: ", ORANGE, vec![]),
                        (&format!("{}", &e), RED, vec![]),
                    ], NewLine);
                }
            }
        }
        let directories = [
            ("uploads", "uploads folder"),
            ("images", "images folder"),
            ("static", "static folder"),
            ("static/audio", "static/audio folder"),
            ("static/media", "static/media folder"),
            ("static/files", "static/files folder"),
            ("static/documents", "static/documents folder"),
            ("static/slides", "static/slides folder"),
            ("static/posts", "static/posts folder"),
        ];
        for (dir, description) in directories.iter() {
            let dir_path = Path::new(dir);
            if !dir_path.exists() {
                match fs::create_dir_all(dir_path) {
                    Ok(_) => {
                        print_fancy(&[
                            ("The ", CYAN, vec![]),
                            (description, VIOLET, vec![]),
                            (" has been ", CYAN, vec![]),
                            ("created.", GREEN, vec![]),
                            (".", CYAN, vec![]),
                        ], NewLine);
                    }
                    Err(e) => {
                        print_fancy(&[
                            (&format!("Error creating {}: ", &description), ORANGE, vec![]),
                            (&format!("{}", &e), RED, vec![]),
                        ], NewLine);
                    }
                }
            } else {
                print_fancy(&[
                    (&format!("{} already exists", &description), YELLOW, vec![]),
                ], NewLine);
            }
        }
        let text_files = [
            ("static/home.html", EXAMPLE_HOME, "home.html"),
            ("static/stuff.html", EXAMPLE_STUFF, "stuff.html"),
            ("static/thumb.html", EXAMPLE_THUMB, "thumb.html"),
            ("static/pdf.html", EXAMPLE_PDF, "pdf.html"),
            ("static/blog.html", EXAMPLE_BLOG, "blog.html"),
            ("static/downloads.html", EXAMPLE_DOWNLOADS, "downloads.html"),
            ("static/playlists.html", PLAYLISTS, "playlists.html"),
            ("static/upload.html", UPLOAD, "upload.html"),
            ("static/uploads.html", FILES, "uploads.html"),
            ("static/error.html", EXAMPLE_ERROR, "error.html"),
            ("static/slides/0.md", EXAMPLE_FIRST_SLIDE, "0.md"),
            ("static/slides/1.md", EXAMPLE_SECOND_SLIDE, "1.md"),
            ("static/posts/Rust.md", EXAMPLE_FIRST_POST, "Rust.md"),
            ("static/posts/Python.md", EXAMPLE_SECOND_POST, "Python.md"),
        ];
        for (file_path, contents, filename) in text_files.iter() {
            let path = Path::new(file_path);
            if !check_file_exists(path, filename) {
                match fs::write(path, *contents) {
                    Ok(_) => {
                        print_fancy(&[
                            ("Example ", CYAN, vec![]),
                            (filename, VIOLET, vec![]),
                            (" file has been ", CYAN, vec![]),
                            ("created.", GREEN, vec![]),
                            (".", CYAN, vec![]),
                        ], NewLine);
                    }
                    Err(e) => {
                        print_fancy(&[
                            ("Failed to create example ", ORANGE, vec![]),
                            (filename, VIOLET, vec![]),
                            (" file: ", ORANGE, vec![]),
                            (&format!("{}", &e), RED, vec![]),
                        ], NewLine);
                    }
                }
            }
        }
        let binary_files = [
            ("static/media/qrcode.png", IMAGE_DATA, "qrcode.png"),
            ("static/documents/example.pdf", PDF_DATA, "example.pdf"),
        ];
        for (file_path, contents, filename) in binary_files.iter() {
            let path = Path::new(file_path);
            if !check_file_exists(path, filename) {
                match fs::write(path, *contents) {
                    Ok(_) => {
                        print_fancy(&[
                            ("Example ", CYAN, vec![]),
                            (filename, VIOLET, vec![]),
                            (" file has been ", CYAN, vec![]),
                            ("created.", GREEN, vec![]),
                            (".", CYAN, vec![]),
                        ], NewLine);
                    }
                    Err(e) => {
                        print_fancy(&[
                            ("Failed to create example ", ORANGE, vec![]),
                            (filename, VIOLET, vec![]),
                            (" file: ", ORANGE, vec![]),
                            (&format!("{}", &e), RED, vec![]),
                        ], NewLine);
                    }
                }
            }
        }
        let path = env::current_dir().expect("Failed to get current directory");
        print_fancy(&[
            ("\nSetup in ", CYAN, vec![]),
            (&format!("{}", path.display()), VIOLET, vec![]),
            (" is ", CYAN, vec![]),
            ("complete", GREEN, vec![]),
            (".\n", CYAN, vec![]),
            ("Please read config and edit ", CYAN, vec![]),
            ("config.toml", VIOLET, vec![]),
            (" to preferences.", CYAN, vec![]),
        ], NewLine);
    }
}
