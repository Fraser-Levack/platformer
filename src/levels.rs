//! Level files on disk: `assets/levels/*.txt`, played in filename order.

use std::fs;
use std::path::{Path, PathBuf};

/// Shipped with the binary so the game still runs if the levels folder is missing.
const DEFAULT_LEVEL: &str = include_str!("../assets/levels/level_01.txt");

pub fn levels_dir() -> PathBuf {
    let local = PathBuf::from("assets/levels");
    if local.is_dir() {
        return local;
    }
    // Running the exe from somewhere else (e.g. target/release): use the project folder.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/levels")
}

/// All level files, sorted. Creates the folder and a default level if needed.
pub fn list() -> Vec<PathBuf> {
    let dir = levels_dir();
    let _ = fs::create_dir_all(&dir);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "txt"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    if files.is_empty() {
        let path = dir.join("level_01.txt");
        if fs::write(&path, DEFAULT_LEVEL).is_ok() {
            files.push(path);
        }
    }
    files
}

pub fn load(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|_| DEFAULT_LEVEL.to_string())
}

pub fn save(path: &Path, text: &str) -> std::io::Result<()> {
    fs::write(path, text)
}

pub fn delete(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)
}

/// Next unused `level_NN.txt` path.
pub fn next_free_path() -> PathBuf {
    let dir = levels_dir();
    (1..)
        .map(|n| dir.join(format!("level_{n:02}.txt")))
        .find(|p| !p.exists())
        .unwrap()
}

pub fn display_name(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}
