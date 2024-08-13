use crate::constants::*;

use std::path::{Path, PathBuf};
use std::io::BufReader;
use std::env;
use std::fs::{self, File};
use std::io;
use zip::ZipArchive;
use solarized::{
    print_fancy, clear,
    VIOLET, BLUE, CYAN, GREEN, ORANGE, RED,
    PrintMode::NewLine,
};

pub fn generate_files() {
    print_fancy(&[
        ("Failed to read configuration\n", ORANGE, vec![]),
        ("Example environment can be created in the current active directory.\n", CYAN, vec![]),
        ("Would you like to create an example environment?\n", CYAN, vec![]),
        ("(", VIOLET, vec![]),
        ("y", BLUE, vec![]),
        ("/", VIOLET, vec![]),
        ("n", RED, vec![]),
        (")", VIOLET, vec![]),
    ], NewLine);
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim().to_lowercase();
    if input == "y" || input == "yes" {
        clear();
        match fs::write("config.toml", EXAMPLE_CONFIG) {
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
        let templates = Path::new("static");
        if !templates.exists() {
            match fs::create_dir_all(&templates) {
                Ok(_) => {
                    print_fancy(&[
                        ("The ", CYAN, vec![]),
                        ("static", VIOLET, vec![]),
                        (" folder has been ", CYAN, vec![]),
                        ("created.", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Error creating static: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
        } else {
            print_fancy(&[
                ("static folder exists", ORANGE, vec![]),
            ], NewLine);
        }
        let audio = Path::new("static/audio");
        if !audio.exists() {
            match fs::create_dir_all(&audio) {
                Ok(_) => {
                    print_fancy(&[
                        ("The ", CYAN, vec![]),
                        ("static", VIOLET, vec![]),
                        (" folder has been ", CYAN, vec![]),
                        ("created.", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Error creating static/audio: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
        }
        let media = Path::new("static/media");
        if !media.exists() {
            match fs::create_dir_all(&media) {
                Ok(_) => {
                    print_fancy(&[
                        ("The ", CYAN, vec![]),
                        ("static/media", VIOLET, vec![]),
                        (" folder has been ", CYAN, vec![]),
                        ("created.", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Error creating static/media: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
        } else {
            println!("media folder exists");
        }
        let uploads = Path::new("uploads");
        if !uploads.exists() {
            match fs::create_dir_all(&uploads) {
                Ok(_) => {
                    print_fancy(&[
                        ("The ", CYAN, vec![]),
                        ("uploads", VIOLET, vec![]),
                        (" folder has been ", CYAN, vec![]),
                        ("created.", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Error creating uploads: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
        } else {
            println!("uploads folder exists");
        }
        let files = Path::new("static/files");
        if !files.exists() {
            match fs::create_dir_all(&files) {
                Ok(_) => {
                    print_fancy(&[
                        ("The ", CYAN, vec![]),
                        ("files", VIOLET, vec![]),
                        (" folder has been ", CYAN, vec![]),
                        ("created.", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => {
                    print_fancy(&[
                        ("Error creating files: ", ORANGE, vec![]),
                        (&format!("{}", e), RED, vec![]),
                    ], NewLine);
                }
            }
        } else {
            println!("files folder exists");
        }
        match fs::write("static/home.html", EXAMPLE_HOME) {
            Ok(_) => {
                print_fancy(&[
                    ("Example ", CYAN, vec![]),
                    ("home.html", VIOLET, vec![]),
                    (" file has been ", CYAN, vec![]),
                    ("created.", GREEN, vec![]),
                    (".", CYAN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    (&format!("{}", e), CYAN, vec![]),
                ], NewLine);
            }
        }
        match fs::write("static/stuff.html", EXAMPLE_STUFF) {
            Ok(_) => {
                print_fancy(&[
                    ("Example ", CYAN, vec![]),
                    ("stuff.html", VIOLET, vec![]),
                    (" file has been ", CYAN, vec![]),
                    ("created.", GREEN, vec![]),
                    (".", CYAN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to create example ", ORANGE, vec![]),
                    ("stuff.html", VIOLET, vec![]),
                    (" file: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
        match fs::write("static/pdf.html", EXAMPLE_PDF) {
            Ok(_) => {
                print_fancy(&[
                    ("Example ", CYAN, vec![]),
                    ("pdf.html", VIOLET, vec![]),
                    (" file has been ", CYAN, vec![]),
                    ("created.", GREEN, vec![]),
                    (".", CYAN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to create example ", ORANGE, vec![]),
                    ("pdf.html", VIOLET, vec![]),
                    (" file: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
        match fs::write("static/downloads.html", EXAMPLE_DOWNLOADS) {
            Ok(_) => {
                print_fancy(&[
                    ("Example ", CYAN, vec![]),
                    ("downloads.html", VIOLET, vec![]),
                    (" file has been ", CYAN, vec![]),
                    ("created.", GREEN, vec![]),
                    (".", CYAN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to create example ", ORANGE, vec![]),
                    ("downloads.html", VIOLET, vec![]),
                    (" file: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
        match fs::write("static/playlists.html", PLAYLISTS) {
            Ok(_) => {
                print_fancy(&[
                    ("Example ", CYAN, vec![]),
                    ("playlists.html", VIOLET, vec![]),
                    (" file has been ", CYAN, vec![]),
                    ("created.", GREEN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to create example ", ORANGE, vec![]),
                    ("playlists.html", VIOLET, vec![]),
                    (" file: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
        match fs::write("static/upload.html", UPLOAD) {
            Ok(_) => {
                print_fancy(&[
                    ("Example ", CYAN, vec![]),
                    ("upload.html", VIOLET, vec![]),
                    (" file has been ", CYAN, vec![]),
                    ("created.", GREEN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to create example ", ORANGE, vec![]),
                    ("upload.html", VIOLET, vec![]),
                    (" file: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
        match fs::write("static/uploads.html", FILES) {
            Ok(_) => {
                print_fancy(&[
                    ("Example ", CYAN, vec![]),
                    ("uploads.html", VIOLET, vec![]),
                    (" file has been ", CYAN, vec![]),
                    ("created.", GREEN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to create example ", ORANGE, vec![]),
                    ("uploads.html", VIOLET, vec![]),
                    (" file: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
        match fs::write("static/error.html", EXAMPLE_ERROR) {
            Ok(_) => {
                print_fancy(&[
                    ("Example ", CYAN, vec![]),
                    ("error.html", VIOLET, vec![]),
                    (" file has been ", CYAN, vec![]),
                    ("created.", GREEN, vec![]),
                    (".", CYAN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to create example ", ORANGE, vec![]),
                    ("error.html", VIOLET, vec![]),
                    (" file: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
        let image_path = "static/media/qrcode.png";
        match std::fs::write(image_path, IMAGE_DATA) {
            Ok(_) => {
                print_fancy(&[
                    ("Image ", CYAN, vec![]),
                    (&format!("{}", image_path), VIOLET, vec![]),
                    (" has been ", CYAN, vec![]),
                    ("saved.", GREEN, vec![]),
                    (".", CYAN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to write image: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
        let pdf_path = "static/documents/asdf.pdf";
        let pdf_dir = Path::new("static/documents");
        if !pdf_dir.exists() {
            match fs::create_dir_all(pdf_dir) {
                Ok(_) => {
                    print_fancy(&[
                        ("The ", CYAN, vec![]),
                        ("static/documents", VIOLET, vec![]),
                        (" folder has been ", CYAN, vec![]),
                        ("created.", GREEN, vec![]),
                        (".", CYAN, vec![]),
                    ], NewLine);
                }
                Err(e) => println!("Error creating static/documents: {:?}", e),
            }
        } else {
            print_fancy(&[
                ("static/documents folder exists", ORANGE, vec![]),
            ], NewLine);
        }
        match std::fs::write(pdf_path, PDF_DATA) {
            Ok(_) => {
                print_fancy(&[
                    ("Document ", CYAN, vec![]),
                    (&format!("{}", pdf_path), VIOLET, vec![]),
                    (" has been ", CYAN, vec![]),
                    ("saved.", GREEN, vec![]),
                    (".", CYAN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to write image: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
        let zip_path = "todos.zip";
        match std::fs::write(zip_path, ARCHIVE_DATA) {
            Ok(_) => {
                print_fancy(&[
                    ("Archive ", CYAN, vec![]),
                    (&format!("{}", zip_path), VIOLET, vec![]),
                    (" has been ", CYAN, vec![]),
                    ("saved.", GREEN, vec![]),
                    (".", CYAN, vec![]),
                ], NewLine);
            }
            Err(e) => {
                print_fancy(&[
                    ("Failed to write image: ", ORANGE, vec![]),
                    (&format!("{}", e), RED, vec![]),
                ], NewLine);
            }
        }
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
        let path = env::current_dir().expect("asdf");
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
