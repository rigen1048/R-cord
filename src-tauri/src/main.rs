#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod player;
mod recorder;

use recorder::SharedRecorder;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct TreeNode {
    name: String,
    path: String,
    children: Vec<TreeNode>,
}

#[derive(Serialize)]
struct Take {
    name: String,
    path: String,
    duration_secs: Option<f64>,
}

fn config_path() -> PathBuf {
    dirs_config().join("rcord.json")
}

fn dirs_config() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
        });
    let dir = base.join("rcord");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn load_vault() -> Option<String> {
    let text = std::fs::read_to_string(config_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let path = v.get("vault")?.as_str()?.to_string();
    if Path::new(&path).is_dir() {
        Some(path)
    } else {
        None
    }
}

#[tauri::command]
fn get_vault() -> Option<String> {
    load_vault()
}

#[tauri::command]
fn set_vault(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.is_dir() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let json = serde_json::json!({ "vault": path });
    std::fs::write(config_path(), json.to_string()).map_err(|e| e.to_string())
}

fn build_tree(dir: &Path, depth: usize) -> Vec<TreeNode> {
    if depth > 16 {
        return vec![];
    }
    let mut nodes = vec![];
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return nodes,
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !p.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true))
        .collect();
    dirs.sort();
    for d in dirs {
        let name = d.file_name().unwrap_or_default().to_string_lossy().to_string();
        let children = build_tree(&d, depth + 1);
        nodes.push(TreeNode {
            name,
            path: d.to_string_lossy().to_string(),
            children,
        });
    }
    nodes
}

#[tauri::command]
fn list_tree(vault: String) -> Vec<TreeNode> {
    build_tree(Path::new(&vault), 0)
}

#[tauri::command]
fn create_folder(parent: String, name: String) -> Result<TreeNode, String> {
    let clean = name.trim();
    if clean.is_empty() || clean.contains(['/', '\\']) || clean.starts_with('.') {
        return Err("invalid folder name".into());
    }
    let dir = Path::new(&parent).join(clean);
    if dir.exists() {
        return Err("folder already exists".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(TreeNode {
        name: clean.to_string(),
        path: dir.to_string_lossy().to_string(),
        children: vec![],
    })
}

#[tauri::command]
fn rename_folder(path: String, new_name: String) -> Result<(), String> {
    let clean = new_name.trim();
    if clean.is_empty() || clean.contains(['/', '\\']) || clean.starts_with('.') {
        return Err("invalid folder name".into());
    }
    let src = Path::new(&path);
    if !src.is_dir() {
        return Err("folder not found".into());
    }
    let dst = src.with_file_name(clean);
    if dst.exists() {
        return Err("a folder with that name already exists".into());
    }
    std::fs::rename(src, dst).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_takes(folder: String) -> Result<Vec<Take>, String> {
    let mut takes = vec![];
    for e in std::fs::read_dir(&folder).map_err(|e| e.to_string())? {
        let p = match e {
            Ok(e) => e.path(),
            Err(_) => continue,
        };
        if p.extension().map(|x| x == "wav").unwrap_or(false) {
            takes.push(Take {
                name: p.file_stem().unwrap_or_default().to_string_lossy().to_string(),
                duration_secs: recorder::wav_duration_secs(&p),
                path: p.to_string_lossy().to_string(),
            });
        }
    }
    takes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(takes)
}

#[tauri::command]
fn rename_take(path: String, new_name: String) -> Result<String, String> {
    let clean = new_name.trim();
    if clean.is_empty() || clean.contains(['/', '\\']) {
        return Err("invalid name".into());
    }
    let src = Path::new(&path);
    if !src.is_file() {
        return Err("take not found".into());
    }
    let dst = src.with_file_name(format!("{clean}.wav"));
    if dst.exists() {
        return Err("a take with that name already exists".into());
    }
    std::fs::rename(src, &dst).map_err(|e| e.to_string())?;
    Ok(dst.to_string_lossy().to_string())
}

#[tauri::command]
fn list_input_devices() -> Vec<recorder::DeviceInfo> {
    recorder::input_devices()
}

#[tauri::command]
fn start_recording(
    state: tauri::State<'_, SharedRecorder>,
    folder: String,
    device: Option<String>,
) -> Result<(), String> {
    state
        .lock()
        .unwrap()
        .start(PathBuf::from(folder), device)
}

#[tauri::command]
fn stop_recording(state: tauri::State<'_, SharedRecorder>) -> Result<Take, String> {
    let path = state.lock().unwrap().stop()?;
    Ok(Take {
        name: path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
        duration_secs: recorder::wav_duration_secs(&path),
        path: path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
fn play_take(state: tauri::State<'_, player::SharedPlayer>, path: String) -> Result<(), String> {
    state.lock().unwrap().play(PathBuf::from(path))
}

#[tauri::command]
fn stop_playback(state: tauri::State<'_, player::SharedPlayer>) {
    state.lock().unwrap().stop();
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(recorder::new_shared())
        .manage(player::new_shared())
        .invoke_handler(tauri::generate_handler![
            get_vault,
            set_vault,
            list_tree,
            create_folder,
            rename_folder,
            list_takes,
            rename_take,
            list_input_devices,
            start_recording,
            stop_recording,
            play_take,
            stop_playback,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rcord");
}
