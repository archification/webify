use std::fs::File;
use std::io;
use std::path::Path;
use walkdir::{DirEntry, WalkDir};
use zip::{write::FileOptions, ZipWriter, CompressionMethod};
use solarized::{
    print_fancy,
    VIOLET, CYAN,
    BOLD, UNDERLINED,
    PrintMode::NewLine,
};

fn should_zip(entry: &DirEntry) -> bool {
    entry.file_type().is_file() || entry.file_type().is_dir()
}

pub fn add_dir_to_zip<P: AsRef<Path>>(src_dir: P, dest_zip: P) -> Result<(), Box<dyn std::error::Error>> {
    let src_path = src_dir.as_ref();
    let file = File::create(&dest_zip)?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);
    let walker = WalkDir::new(&src_path).into_iter();
    for entry in walker.filter_map(|e| e.ok()).filter(should_zip) {
        let path = entry.path();
        let name = path.strip_prefix(&src_path)?;
        if path.is_file() {
            print_fancy(&[
                ("Adding file: ", CYAN, vec![]),
                (&format!("{:?}", path), VIOLET, vec![UNDERLINED]),
            ], NewLine);
            zip.start_file(name.to_string_lossy(), options)?;
            let mut f = File::open(path)?;
            io::copy(&mut f, &mut zip)?;
        } else if entry.file_type().is_dir() && name.as_os_str().len() > 0 {
            print_fancy(&[
                ("Adding directory: ", CYAN, vec![BOLD]),
                (&format!("{:?}", path), VIOLET, vec![UNDERLINED]),
            ], NewLine);
            zip.add_directory(name.to_string_lossy(), options)?;
        }
    }
    zip.finish()?;
    Ok(())
}
