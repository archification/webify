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
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
        }
        let directories = [
            ("static", "static folder"),
            ("static/audio", "static/audio folder"),
            ("static/media", "static/media folder"),
            ("uploads", "uploads folder"),
            ("static/files", "static/files folder"),
            ("static/documents", "static/documents folder"),
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
                            (&format!("Error creating {}: ", description), ORANGE, vec![]),
                            (&format!("{}", e), RED, vec![]),
                        ], NewLine);
                    }
                }
            } else {
                print_fancy(&[
                    (&format!("{} already exists", description), YELLOW, vec![]),
                ], NewLine);
            }
        }
        let text_files = [
            ("static/home.html", EXAMPLE_HOME, "home.html"),
            ("static/stuff.html", EXAMPLE_STUFF, "stuff.html"),
            ("static/pdf.html", EXAMPLE_PDF, "pdf.html"),
            ("static/downloads.html", EXAMPLE_DOWNLOADS, "downloads.html"),
            ("static/playlists.html", PLAYLISTS, "playlists.html"),
            ("static/upload.html", UPLOAD, "upload.html"),
            ("static/uploads.html", FILES, "uploads.html"),
            ("static/error.html", EXAMPLE_ERROR, "error.html"),
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
                            (&format!("{}", e), RED, vec![]),
                        ], NewLine);
                    }
                }
            }
        }
        let binary_files = [
            ("static/media/qrcode.png", IMAGE_DATA, "qrcode.png"),
            ("static/documents/asdf.pdf", PDF_DATA, "asdf.pdf"),
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
                            (&format!("{}", e), RED, vec![]),
                        ], NewLine);
                    }
                }
            }
        }

        /*
        // Extract ZIP file
        let file_path = Path::new("todos.zip");
        let file = File::open(&file_path).expect("Failed to open ZIP file");
        let mut archive = ZipArchive::new(BufReader::new(file)).expect("Failed to read ZIP archive");
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).expect("Failed to access file in ZIP archive");
            let file_name = file.name().to_string();
            fn construct_safe_path(file_name: &str) -> PathBuf {
                let mut path = PathBuf::new();
                for component in Path::new(file_name).components() {
                    match component {
                        std::path::Component::Normal(comp) => path.push(comp),
                        _ => {}
                    }
                }
                path
            }
            let outpath = construct_safe_path(&file_name);
            if file_name.ends_with('/') {
                print_fancy(&[
                    ("Directory ", CYAN, vec![]),
                    ("extracted", GREEN, vec![]),
                    (" to ", CYAN, vec![]),
                    (&format!("{}", outpath.display()), VIOLET, vec![]),
                ], NewLine);
                std::fs::create_dir_all(&outpath).expect("Failed to create directory");
            } else {
                print_fancy(&[
                    ("File ", CYAN, vec![]),
                    (&format!("{}", i), VIOLET, vec![]),
                    (" extracted ", GREEN, vec![]),
                    ("to ", CYAN, vec![]),
                    (&format!("{}", outpath.display()), VIOLET, vec![]),
                ], NewLine);
                if let Some(parent) = outpath.parent() {
                    std::fs::create_dir_all(parent).expect("Failed to create directory");
                }
                let mut outfile = std::fs::File::create(&outpath).expect("Failed to create file");
                std::io::copy(&mut file, &mut outfile).expect("Failed to copy file");
            }
        }
        print_fancy(&[
            ("ZIP archive ", CYAN, vec![]),
            ("extracted", GREEN, vec![]),
        ], NewLine);
        std::fs::remove_file(file_path).expect("Failed to delete ZIP file");
        print_fancy(&[
            ("ZIP file deleted ", CYAN, vec![]),
            ("successfully", GREEN, vec![]),
        ], NewLine);
*/

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
