#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Utc};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::SystemTime,
};
use tauri::{Emitter, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder};
#[cfg(target_os = "macos")]
use tauri::{LogicalPosition, TitleBarStyle};

struct AppState {
    watchers: Mutex<HashMap<String, RecommendedWatcher>>,
}

#[derive(Serialize)]
struct AppMeta {
    name: String,
    version: String,
}

#[derive(Serialize)]
struct FileNode {
    #[serde(rename = "type")]
    node_type: String,
    name: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<Vec<FileNode>>,
}

#[derive(Serialize)]
struct FileResult {
    path: String,
    name: String,
}

#[derive(Serialize)]
struct FileStat {
    mtime: String,
    size: u64,
}

#[derive(Serialize)]
struct FileBase64 {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct TemplateFile {
    name: String,
    path: String,
    content: String,
}

#[derive(Serialize, Clone)]
struct FsChange {
    event: String,
    path: String,
}

#[derive(Serialize)]
struct TerminalResult {
    stdout: String,
    stderr: String,
    code: i32,
    error: Option<String>,
    #[serde(rename = "durationMs")]
    duration_ms: u64,
}

#[derive(Serialize)]
struct LocalAiTool {
    id: String,
    label: String,
    command: String,
    path: Option<String>,
    version: Option<String>,
    available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellInfo {
    path: String,
    name: String,
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn terminal_shell() -> String {
    if cfg!(windows) {
        std::env::var("ComSpec").unwrap_or_else(|_| "cmd".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    }
}

fn markdown_file(path: &Path) -> bool {
    path.extension()
        .map(|ext| {
            let ext = ext.to_string_lossy();
            ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
        })
        .unwrap_or(false)
}

fn project_visible_file(path: &Path) -> bool {
    if markdown_file(path) {
        return true;
    }
    let name = path.to_string_lossy().to_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".bmp", ".pdf",
        ".fcanvas.json", ".fdraw.json",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
}

fn launch_file_from_args() -> Option<String> {
    std::env::args().skip(1).find_map(|arg| {
        let path = PathBuf::from(arg);
        if path.is_file() && markdown_file(&path) {
            Some(path_string(&path))
        } else {
            None
        }
    })
}

fn open_url_file_path(url: &Url) -> Option<String> {
    let path = url.to_file_path().ok()?;
    if path.is_file() && markdown_file(&path) {
        Some(path_string(&path))
    } else {
        None
    }
}

fn read_folder_tree(folder_path: &Path) -> Vec<FileNode> {
    let mut entries = Vec::new();
    let Ok(items) = fs::read_dir(folder_path) else {
        return entries;
    };

    for item in items.flatten() {
        let name = item.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let full_path = item.path();
        if full_path.is_dir() {
            entries.push(FileNode {
                node_type: "folder".into(),
                name,
                path: path_string(&full_path),
                children: Some(read_folder_tree(&full_path)),
            });
        } else if project_visible_file(&full_path) {
            entries.push(FileNode {
                node_type: "file".into(),
                name,
                path: path_string(&full_path),
                children: None,
            });
        }
    }

    entries.sort_by(|a, b| match (a.node_type.as_str(), b.node_type.as_str()) {
        ("folder", "file") => std::cmp::Ordering::Less,
        ("file", "folder") => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    entries
}

fn notify_event_name(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::Create(_) => "add",
        EventKind::Remove(_) => "unlink",
        EventKind::Modify(_) => "change",
        _ => "change",
    }
}

fn unique_copy_path(file_path: &Path) -> PathBuf {
    let ext = file_path.extension().and_then(|v| v.to_str()).unwrap_or("");
    let stem = file_path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("copy");
    let dir = file_path.parent().unwrap_or_else(|| Path::new(""));
    let suffix = if ext.is_empty() {
        String::new()
    } else {
        format!(".{ext}")
    };
    let mut copy_path = dir.join(format!("{stem}-copy{suffix}"));
    let mut counter = 1;

    while copy_path.exists() {
        counter += 1;
        copy_path = dir.join(format!("{stem}-copy-{counter}{suffix}"));
    }
    copy_path
}

fn export_html_document(file_name: &str, html: &str, theme: &str, settings: &Value) -> String {
    let is_light = theme == "light";
    let bg = if is_light { "#f7f2e8" } else { "#0d0e10" };
    let surface = if is_light { "#fffaf0" } else { "#121417" };
    let text = if is_light { "#221d18" } else { "#dddfe6" };
    let muted = if is_light { "#5f564b" } else { "#7a7d8a" };
    let border = if is_light {
        "rgba(34,29,24,0.12)"
    } else {
        "rgba(255,255,255,0.08)"
    };
    let accent = if is_light { "#5c7695" } else { "#5b7fa6" };
    let font = settings
        .get("previewFontCustom")
        .or_else(|| settings.get("previewFont"))
        .and_then(Value::as_str)
        .unwrap_or("'DM Sans', system-ui, sans-serif");
    let font_size = settings
        .get("previewFontSize")
        .and_then(Value::as_i64)
        .unwrap_or(13);
    let line_height = settings
        .get("previewLineHeight")
        .and_then(Value::as_f64)
        .unwrap_or(1.75);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{file_name}</title>
  <style>
    :root {{ --bg: {bg}; --surface: {surface}; --text: {text}; --muted: {muted}; --border: {border}; --accent: {accent}; --font: {font}; --font-size: {font_size}px; --line-height: {line_height}; }}
    * {{ box-sizing: border-box; }}
    html, body {{ margin: 0; padding: 0; background: var(--bg); color: var(--text); }}
    body {{ font-family: var(--font); font-size: var(--font-size); line-height: var(--line-height); }}
    .page {{ max-width: 860px; margin: 0 auto; padding: 48px 56px 72px; background: var(--surface); }}
    .preview-pane h1 {{ font-size: 1.7em; font-weight: 600; border-bottom: 1px solid var(--border); padding-bottom: 10px; margin: 0 0 14px; }}
    .preview-pane h2 {{ font-size: 1.3em; font-weight: 600; margin: 24px 0 8px; }}
    .preview-pane h3 {{ font-size: 1.08em; font-weight: 600; color: var(--muted); margin: 18px 0 6px; }}
    .preview-pane p {{ margin: 0 0 12px; white-space: pre-wrap; }}
    .preview-pane ul, .preview-pane ol {{ padding-left: 18px; margin: 0 0 12px; }}
    .preview-pane code {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.92em; background: rgba(127,127,127,0.14); padding: 1px 5px; border-radius: 4px; color: var(--accent); }}
    .preview-pane pre {{ background: rgba(127,127,127,0.1); border: 1px solid var(--border); border-radius: 8px; padding: 14px 16px; overflow-x: auto; }}
    .preview-pane blockquote {{ border-left: 2px solid var(--border); margin: 14px 0; padding: 3px 0 3px 14px; color: var(--muted); }}
    .preview-pane a {{ color: var(--accent); text-decoration: none; }}
    .preview-pane img {{ max-width: 100%; border-radius: 4px; margin: 8px 0; }}
    .preview-pane table {{ border-collapse: collapse; width: 100%; margin: 12px 0; }}
    .preview-pane th, .preview-pane td {{ padding: 5px 10px; border-bottom: 1px solid var(--border); text-align: left; }}
  </style>
</head>
<body><main class="page"><article class="preview-pane">{html}</article></main></body>
</html>"#
    )
}

#[tauri::command]
fn app_meta(app: tauri::AppHandle) -> AppMeta {
    AppMeta {
        name: "Rísta".into(),
        version: app.package_info().version.to_string(),
    }
}

#[tauri::command]
fn launch_file() -> Option<String> {
    launch_file_from_args()
}

#[tauri::command]
fn read_folder(path: String) -> Vec<FileNode> {
    read_folder_tree(Path::new(&path))
}

#[tauri::command]
fn read_file(path: String) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[tauri::command]
fn read_file_base64(path: String) -> FileBase64 {
    match fs::read(path) {
        Ok(bytes) => FileBase64 {
            mime_type: "application/octet-stream".into(),
            data: Some(general_purpose::STANDARD.encode(bytes)),
            error: None,
        },
        Err(err) => FileBase64 {
            mime_type: "application/octet-stream".into(),
            data: None,
            error: Some(err.to_string()),
        },
    }
}

#[tauri::command]
fn write_file(path: String, content: String) -> bool {
    fs::write(path, content).is_ok()
}

#[tauri::command]
fn save_binary_file(path: String, base64_data: String) -> bool {
    let Ok(bytes) = general_purpose::STANDARD.decode(base64_data) else {
        return false;
    };
    fs::write(path, bytes).is_ok()
}

#[tauri::command]
fn create_dir(path: String) -> bool {
    fs::create_dir_all(path).is_ok()
}

#[tauri::command]
fn write_image_file(
    dir_path: String,
    base64_data: String,
    file_name: String,
) -> Option<FileResult> {
    let dir = PathBuf::from(dir_path);
    fs::create_dir_all(&dir).ok()?;
    let bytes = general_purpose::STANDARD.decode(base64_data).ok()?;
    let path = dir.join(&file_name);
    fs::write(&path, bytes).ok()?;
    Some(FileResult {
        path: path_string(&path),
        name: file_name,
    })
}

#[tauri::command]
fn create_file(path: String) -> Option<FileResult> {
    let path = PathBuf::from(path);
    if !path.exists() && fs::write(&path, "").is_err() {
        return None;
    }
    Some(FileResult {
        name: basename(&path),
        path: path_string(&path),
    })
}

#[tauri::command]
fn rename_file(old_path: String, new_path: String) -> bool {
    fs::rename(old_path, new_path).is_ok()
}

#[tauri::command]
fn trash_file(path: String) -> bool {
    trash::delete(path).is_ok()
}

#[tauri::command]
fn delete_file(path: String) -> bool {
    fs::remove_file(path).is_ok()
}

#[tauri::command]
fn duplicate_file(path: String) -> Option<FileResult> {
    let source = PathBuf::from(path);
    let copy_path = unique_copy_path(&source);
    fs::copy(&source, &copy_path).ok()?;
    Some(FileResult {
        name: basename(&copy_path),
        path: path_string(&copy_path),
    })
}

#[tauri::command]
fn stat_file(path: String) -> Option<FileStat> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().unwrap_or_else(|_| SystemTime::now());
    let datetime: DateTime<Utc> = modified.into();
    Some(FileStat {
        mtime: datetime.to_rfc3339(),
        size: meta.len(),
    })
}

#[tauri::command]
fn read_templates(folder_path: String) -> Vec<TemplateFile> {
    let templates_dir = Path::new(&folder_path).join("_templates");
    let Ok(entries) = fs::read_dir(&templates_dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !markdown_file(&path) {
                return None;
            }
            Some(TemplateFile {
                name: path.file_stem()?.to_string_lossy().to_string(),
                path: path_string(&path),
                content: fs::read_to_string(&path).ok()?,
            })
        })
        .collect()
}

#[tauri::command]
fn list_dir(path: String) -> Vec<String> {
    fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
fn watch_folder(window: tauri::Window, state: tauri::State<AppState>, path: String) -> bool {
    let root = PathBuf::from(path);
    let label = window.label().to_string();
    let window_for_watcher = window.clone();
    let Ok(mut watcher) =
        notify::recommended_watcher(move |result: Result<Event, notify::Error>| {
            if let Ok(event) = result {
                let event_name = notify_event_name(&event.kind).to_string();
                for changed_path in event.paths {
                    if markdown_file(&changed_path) {
                        let _ = window_for_watcher.emit(
                            "fs-change",
                            FsChange {
                                event: event_name.clone(),
                                path: path_string(&changed_path),
                            },
                        );
                    }
                }
            }
        })
    else {
        return false;
    };

    if watcher.watch(&root, RecursiveMode::Recursive).is_err() {
        return false;
    }

    if let Ok(mut watchers) = state.watchers.lock() {
        watchers.insert(label, watcher);
        true
    } else {
        false
    }
}

fn open_markdown_file_in_window(app: tauri::AppHandle, file_path: String) -> Result<bool, String> {
    create_window(app, None, Some(file_path))
}

#[tauri::command]
fn start_window_drag(window: tauri::Window) -> Result<bool, String> {
    window.start_dragging().map(|_| true).map_err(|err| err.to_string())
}

fn create_window(
    app: tauri::AppHandle,
    folder_path: Option<String>,
    file_path: Option<String>,
) -> Result<bool, String> {
    let label = format!("workspace-{}", Utc::now().timestamp_millis());
    let title = file_path
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        .map(|name| format!("{} - Rísta", name.to_string_lossy()))
        .unwrap_or_else(|| "Rísta".into());
    let mut builder = WebviewWindowBuilder::new(&app, label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(1280.0, 820.0)
        .min_inner_size(860.0, 540.0)
        .resizable(true)
        .decorations(true)
        .shadow(true)
        .center();

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(TitleBarStyle::Overlay)
            .hidden_title(true)
            .traffic_light_position(LogicalPosition::new(15.0, 17.0));
    }

    if let Some(folder_path) = folder_path {
        let encoded = serde_json::to_string(&folder_path).map_err(|err| err.to_string())?;
        builder =
            builder.initialization_script(format!("window.__RISTA_INITIAL_FOLDER__ = {encoded};"));
    }
    if let Some(file_path) = file_path {
        let encoded = serde_json::to_string(&file_path).map_err(|err| err.to_string())?;
        builder =
            builder.initialization_script(format!("window.__RISTA_INITIAL_FILE__ = {encoded};"));
    }

    builder.build().map_err(|err| err.to_string())?;
    Ok(true)
}

#[tauri::command]
async fn new_window(
    app: tauri::AppHandle,
    folder_path: Option<String>,
    file_path: Option<String>,
) -> Result<bool, String> {
    create_window(app, folder_path, file_path)
}

#[tauri::command]
fn run_terminal_command(command: String, cwd: Option<String>) -> TerminalResult {
    let started = std::time::Instant::now();
    let shell = terminal_shell();
    let mut cmd = Command::new(&shell);
    if cfg!(windows) {
        cmd.args(["/C", &command]);
    } else {
        cmd.args(["-lc", &command]);
    }
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    match cmd.output() {
        Ok(output) => {
            let duration_ms = started.elapsed().as_millis() as u64;
            TerminalResult {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                code: output.status.code().unwrap_or(-1),
                error: None,
                duration_ms,
            }
        }
        Err(err) => {
            let duration_ms = started.elapsed().as_millis() as u64;
            TerminalResult {
                stdout: String::new(),
                stderr: String::new(),
                code: -1,
                error: Some(err.to_string()),
                duration_ms,
            }
        }
    }
}

#[tauri::command]
fn get_shell_info() -> ShellInfo {
    let path = terminal_shell();
    let name = Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&path)
        .to_string();
    ShellInfo { path, name }
}

#[tauri::command]
fn render_d2(source: String, theme_id: Option<u16>) -> Value {
    let mut child = match Command::new("d2")
        .args(["-", "-", "--theme", &theme_id.unwrap_or(0).to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return json!({ "error": "D2 is not installed. Install from https://d2lang.com" }),
    };

    if let Some(stdin) = child.stdin.as_mut() {
        if stdin.write_all(source.as_bytes()).is_err() {
            return json!({ "error": "Failed to write D2 source" });
        }
    }

    match child.wait_with_output() {
        Ok(output) if output.status.success() => {
            json!({ "svg": String::from_utf8_lossy(&output.stdout).to_string() })
        }
        Ok(output) => json!({ "error": String::from_utf8_lossy(&output.stderr).to_string() }),
        Err(err) => json!({ "error": err.to_string() }),
    }
}

#[tauri::command]
fn export_html(path: String, payload: Value) -> bool {
    let file_name = payload
        .get("fileName")
        .and_then(Value::as_str)
        .unwrap_or("export");
    let html = payload.get("html").and_then(Value::as_str).unwrap_or("");
    let theme = payload
        .get("theme")
        .and_then(Value::as_str)
        .unwrap_or("dark");
    let settings = payload.get("settings").unwrap_or(&Value::Null);
    let document = export_html_document(file_name, html, theme, settings);
    fs::write(path, document).is_ok()
}

#[tauri::command]
fn export_site(output_dir: String, files: Vec<Value>) -> bool {
    let output = PathBuf::from(output_dir);
    if fs::create_dir_all(&output).is_err() {
        return false;
    }
    for file in files {
        let Some(name) = file.get("name").and_then(Value::as_str) else {
            return false;
        };
        let Some(html) = file.get("html").and_then(Value::as_str) else {
            return false;
        };
        if fs::write(output.join(name), html).is_err() {
            return false;
        }
    }
    true
}

#[tauri::command]
fn export_pdf() -> bool {
    false
}

#[tauri::command]
fn import_content(folder_path: String, title: String, body: String, source_url: String) -> Value {
    let safe_title = title
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || ch.is_whitespace())
        .collect::<String>()
        .trim()
        .to_string();
    let base = if safe_title.is_empty() {
        "untitled".into()
    } else {
        safe_title
    };
    let mut file_name = format!("{base}.md");
    let mut file_path = Path::new(&folder_path).join(&file_name);
    let mut counter = 1;

    while file_path.exists() {
        file_name = format!("{base}-{counter}.md");
        file_path = Path::new(&folder_path).join(&file_name);
        counter += 1;
    }

    let content = format!(
        "---\ntitle: {}\nsource: {}\nimported: {}\n---\n\n{}",
        title,
        source_url,
        Utc::now().to_rfc3339(),
        body
    );

    if fs::write(&file_path, content).is_err() {
        return json!({ "error": "Failed to import content" });
    }

    json!({ "path": path_string(&file_path), "name": file_name })
}

#[tauri::command]
fn discover_local_ai_tools() -> Vec<LocalAiTool> {
    let probes = [
        ("codex", "Codex CLI", "codex", "codex --version"),
        ("opencode", "opencode", "opencode", "opencode --version"),
    ];

    probes
        .iter()
        .map(|(id, label, command, _version_probe)| {
            let path = which_command(command);
            let available = path.is_some();
            let version = path.as_deref().and_then(version_command);
            LocalAiTool {
                id: (*id).to_string(),
                label: (*label).to_string(),
                command: (*command).to_string(),
                path,
                version,
                available,
            }
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn which_command(command: &str) -> Option<String> {
    let output = Command::new("where").arg(command).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(not(target_os = "windows"))]
fn which_command(command: &str) -> Option<String> {
    let quoted = command.replace('\'', "'\\''");
    let script = format!("command -v '{}'", quoted);
    let output = Command::new("sh").arg("-lc").arg(script).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn version_command(command: &str) -> Option<String> {
    let output = Command::new(command).arg("--version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version = format!("{}{}", stdout.trim(), stderr.trim()).trim().to_string();
    if output.status.success() && !version.is_empty() {
        Some(version)
    } else {
        None
    }
}

#[tauri::command]
async fn ai_chat(params: Value) -> Value {
    let provider = params.get("provider").and_then(Value::as_str).unwrap_or("");
    let base_url = params.get("baseUrl").and_then(Value::as_str).unwrap_or("");
    let api_key = params.get("apiKey").and_then(Value::as_str).unwrap_or("");
    let model = params
        .get("model")
        .cloned()
        .unwrap_or(Value::String(String::new()));
    let messages = params.get("messages").cloned().unwrap_or(json!([]));
    let tools = params.get("tools").cloned();
    let client = reqwest::Client::new();

    let (url, payload, request) = match provider {
        "anthropic" => {
            let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
            let mut payload = json!({
                "model": model,
                "max_tokens": 4096,
                "messages": messages.as_array().cloned().unwrap_or_default().into_iter().filter(|m| m.get("role").and_then(Value::as_str) != Some("system")).collect::<Vec<_>>()
            });
            if let Some(tools) = tools {
                payload["tools"] = tools;
            }
            let request = client
                .post(&url)
                .header("content-type", "application/json")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01");
            (url, payload, request)
        }
        "openai" | "openrouter" | "custom-openai-compatible" => {
            let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
            let mut payload = json!({ "model": model, "messages": messages });
            if let Some(tools) = tools {
                payload["tools"] = tools;
            }
            let request = client
                .post(&url)
                .header("content-type", "application/json")
                .bearer_auth(api_key);
            (url, payload, request)
        }
        "ollama" => {
            let url = format!("{}/api/chat", base_url.trim_end_matches('/'));
            let payload = json!({ "model": model, "messages": messages, "stream": false });
            let request = client.post(&url).header("content-type", "application/json");
            (url, payload, request)
        }
        _ => return json!({ "error": format!("Unknown provider: {provider}") }),
    };

    let response = match request.json(&payload).send().await {
        Ok(response) => response,
        Err(err) => return json!({ "error": err.to_string(), "url": url }),
    };
    let status = response.status();
    let json_value: Value = match response.json().await {
        Ok(value) => value,
        Err(err) => return json!({ "error": format!("Failed to parse response: {err}") }),
    };
    if !status.is_success() {
        return json!({ "error": json_value.get("error").cloned().unwrap_or(json!(format!("HTTP {status}"))) });
    }

    match provider {
        "openai" | "openrouter" | "custom-openai-compatible" => {
            let message = json_value
                .pointer("/choices/0/message")
                .cloned()
                .unwrap_or(json!({}));
            json!({
                "text": message.get("content").and_then(Value::as_str).unwrap_or(""),
                "stop": json_value.pointer("/choices/0/finish_reason").cloned().unwrap_or(Value::Null)
            })
        }
        "anthropic" => {
            let text = json_value
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| block.get("text").and_then(Value::as_str))
                        .collect::<String>()
                })
                .unwrap_or_default();
            json!({ "text": text, "stop": json_value.get("stop_reason").cloned().unwrap_or(Value::Null) })
        }
        "ollama" => {
            json!({ "text": json_value.pointer("/message/content").and_then(Value::as_str).unwrap_or("") })
        }
        _ => json!({ "error": "Unknown provider" }),
    }
}

#[tauri::command]
fn set_represented_file(_path: Option<String>) -> bool {
    true
}

fn app_icon_bytes(variant: &str, theme: &str) -> Option<&'static [u8]> {
    match (variant, theme) {
        ("nordic-steel", "dark") => Some(include_bytes!(
            "../icons/rista-split-rune-nordic-steel-dark.png"
        )),
        ("nordic-steel", "light") => Some(include_bytes!(
            "../icons/rista-split-rune-nordic-steel-light.png"
        )),
        ("aurora-gradient", "dark") => Some(include_bytes!(
            "../icons/rista-split-rune-aurora-gradient-dark.png"
        )),
        ("aurora-gradient", "light") => Some(include_bytes!(
            "../icons/rista-split-rune-aurora-gradient-light.png"
        )),
        ("black-stone", "dark") => Some(include_bytes!(
            "../icons/rista-split-rune-black-stone-dark.png"
        )),
        ("black-stone", "light") => Some(include_bytes!(
            "../icons/rista-split-rune-black-stone-light.png"
        )),
        ("paper-ink", "dark") => Some(include_bytes!(
            "../icons/rista-split-rune-paper-ink-dark.png"
        )),
        ("paper-ink", "light") => Some(include_bytes!(
            "../icons/rista-split-rune-paper-ink-light.png"
        )),
        ("future-rune", "dark") => Some(include_bytes!(
            "../icons/rista-split-rune-future-rune-dark.png"
        )),
        ("future-rune", "light") => Some(include_bytes!(
            "../icons/rista-split-rune-future-rune-light.png"
        )),
        _ => None,
    }
}

#[tauri::command]
fn set_app_icon(app: tauri::AppHandle, variant: String, theme: String) -> Result<bool, String> {
    let theme = match theme.as_str() {
        "light" => "light",
        _ => "dark",
    };
    let variant = match variant.as_str() {
        "nordic-steel" | "aurora-gradient" | "black-stone" | "paper-ink" | "future-rune" => {
            variant.as_str()
        }
        _ => "aurora-gradient",
    };
    let bytes = app_icon_bytes(variant, theme).ok_or_else(|| "Unknown app icon".to_string())?;
    let image = tauri::image::Image::from_bytes(bytes).map_err(|err| err.to_string())?;
    let mut applied = false;
    for (_, window) in app.webview_windows() {
        window
            .set_icon(image.clone())
            .map_err(|err| err.to_string())?;
        applied = true;
    }
    Ok(applied)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            watchers: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            app_meta,
            read_folder,
            read_file,
            read_file_base64,
            write_file,
            save_binary_file,
            create_dir,
            write_image_file,
            create_file,
            rename_file,
            trash_file,
            delete_file,
            duplicate_file,
            stat_file,
            read_templates,
            list_dir,
            watch_folder,
            launch_file,
            new_window,
            start_window_drag,
            run_terminal_command,
            get_shell_info,
            render_d2,
            export_html,
            export_site,
            export_pdf,
            import_content,
            discover_local_ai_tools,
            ai_chat,
            set_represented_file,
            set_app_icon
        ])
        .build(tauri::generate_context!())
        .expect("error while building Rísta")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            match event {
                RunEvent::Opened { urls } => {
                    for url in urls {
                        if let Some(file_path) = open_url_file_path(&url) {
                            let _ = open_markdown_file_in_window(app.clone(), file_path);
                        }
                    }
                }
                RunEvent::Reopen {
                    has_visible_windows,
                    ..
                } => {
                    if !has_visible_windows && app.webview_windows().is_empty() {
                        let _ = create_window(app.clone(), None, None);
                    }
                }
                _ => {}
            }
        });
}
