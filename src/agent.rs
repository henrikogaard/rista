use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
const STDERR_TAIL_BYTES: usize = 8 * 1024;
pub const MAX_MODEL_CHOICES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub value: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelState {
    pub config_id: Option<String>,
    pub choices: Vec<ModelChoice>,
    pub selected: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingModelSwitch {
    pub request_id: u64,
    pub requested_model_id: String,
    pub config_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelSelection {
    pub state: ModelState,
    pub pending: Option<PendingModelSwitch>,
}

impl ModelSelection {
    pub fn begin(&mut self, request_id: u64, requested_model_id: String) -> bool {
        if self.pending.is_some() {
            return false;
        }
        self.pending = Some(PendingModelSwitch {
            request_id,
            requested_model_id,
            config_id: self.state.config_id.clone(),
        });
        true
    }

    pub fn complete(&mut self, request_id: u64, selected: Option<String>) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        if pending.request_id != request_id {
            return false;
        }
        self.pending = None;
        if let Some(selected) = selected {
            self.state.selected = Some(selected);
        }
        true
    }

    pub fn fail(&mut self, request_id: u64) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.request_id == request_id)
        {
            self.pending = None;
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub fn can_prompt(
    ready: bool,
    has_process: bool,
    has_session: bool,
    stopping: bool,
    model_switch_pending: bool,
    has_usable_model: bool,
    prompt: &str,
) -> bool {
    ready
        && has_process
        && has_session
        && !stopping
        && !model_switch_pending
        && has_usable_model
        && !prompt.trim().is_empty()
}

pub fn can_select_agent(has_process: bool) -> bool {
    !has_process
}

pub fn model_state_from_session(result: &Value) -> ModelState {
    if let Some(options) = result.get("configOptions").and_then(Value::as_array) {
        if let Some(state) = model_state_from_config_options(options) {
            return state;
        }
    }
    let Some(models) = result.get("models") else {
        return ModelState::default();
    };
    let Some(available) = models.get("availableModels").and_then(Value::as_array) else {
        return ModelState::default();
    };
    let mut choices = Vec::new();
    let mut seen = HashSet::new();
    for model in available {
        let (Some(value), Some(name)) = (
            model.get("modelId").and_then(Value::as_str),
            model.get("name").and_then(Value::as_str),
        ) else {
            continue;
        };
        push_model_choice(&mut choices, &mut seen, value, name);
    }
    ModelState {
        config_id: None,
        choices,
        selected: models
            .get("currentModelId")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

pub fn model_state_from_config_options(options: &[Value]) -> Option<ModelState> {
    let config = options.iter().find(|option| {
        option.get("type").and_then(Value::as_str) == Some("select")
            && (option.get("category").and_then(Value::as_str) == Some("model")
                || option.get("id").and_then(Value::as_str) == Some("model"))
    })?;
    let config_id = config.get("id").and_then(Value::as_str)?.to_owned();
    let mut choices = Vec::new();
    let mut seen = HashSet::new();
    if let Some(options) = config.get("options").and_then(Value::as_array) {
        flatten_model_options(options, &mut choices, &mut seen);
    }
    Some(ModelState {
        config_id: Some(config_id),
        choices,
        selected: config
            .get("currentValue")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn flatten_model_options(
    options: &[Value],
    choices: &mut Vec<ModelChoice>,
    seen: &mut HashSet<String>,
) {
    for option in options {
        if let (Some(value), Some(name)) = (
            option.get("value").and_then(Value::as_str),
            option.get("name").and_then(Value::as_str),
        ) {
            push_model_choice(choices, seen, value, name);
        } else if let Some(grouped) = option.get("options").and_then(Value::as_array) {
            flatten_model_options(grouped, choices, seen);
        }
    }
}

fn push_model_choice(
    choices: &mut Vec<ModelChoice>,
    seen: &mut HashSet<String>,
    value: &str,
    name: &str,
) {
    if !value.is_empty() && !name.is_empty() && seen.insert(value.to_owned()) {
        choices.push(ModelChoice {
            value: value.to_owned(),
            name: name.to_owned(),
        });
    }
}

pub fn allowed_model_choices(state: &ModelState, xai_only: bool) -> Vec<ModelChoice> {
    state
        .choices
        .iter()
        .filter(|choice| !xai_only || choice.value.starts_with("xai/"))
        .take(MAX_MODEL_CHOICES)
        .cloned()
        .collect()
}

pub fn has_usable_model(state: &ModelState, xai_only: bool) -> bool {
    !xai_only
        || state.selected.as_ref().is_some_and(|selected| {
            state.choices.iter().any(|choice| {
                choice.value.as_str() == selected.as_str() && choice.value.starts_with("xai/")
            })
        })
}

pub fn selected_model_name(state: &ModelState, xai_only: bool) -> Option<&str> {
    let selected = state.selected.as_deref()?;
    state
        .choices
        .iter()
        .find(|choice| choice.value == selected && (!xai_only || choice.value.starts_with("xai/")))
        .map(|choice| choice.name.as_str())
}

pub fn model_switch_request(
    request_id: u64,
    session_id: &str,
    config_id: Option<&str>,
    model_id: &str,
) -> Value {
    match config_id {
        Some(config_id) => rpc_request(
            request_id,
            "session/set_config_option",
            json!({"sessionId":session_id,"configId":config_id,"value":model_id}),
        ),
        None => rpc_request(
            request_id,
            "session/set_model",
            json!({"sessionId":session_id,"modelId":model_id}),
        ),
    }
}

pub fn config_result_model_value(result: &Value, config_id: &str) -> Option<String> {
    let option = result
        .get("configOptions")
        .and_then(Value::as_array)
        .and_then(|options| {
            options
                .iter()
                .find(|option| option.get("id").and_then(Value::as_str) == Some(config_id))
        })
        .or_else(|| {
            result
                .get("configOption")
                .filter(|option| option.get("id").and_then(Value::as_str) == Some(config_id))
        })
        .or_else(|| {
            (result.get("id").and_then(Value::as_str) == Some(config_id)).then_some(result)
        })?;
    option
        .get("currentValue")
        .or_else(|| option.get("value"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub fn session_update<'a>(params: &'a Value, session_id: &str) -> Option<&'a Value> {
    if params.get("sessionId").and_then(Value::as_str) != Some(session_id) {
        return None;
    }
    params.get("update")
}

pub fn config_options_update(update: &Value) -> Option<&[Value]> {
    (update.get("sessionUpdate").and_then(Value::as_str) == Some("config_option_update"))
        .then(|| {
            update
                .get("configOptions")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
        })
        .flatten()
}

pub struct AgentProcess {
    child: Mutex<Child>,
    outgoing: SyncSender<String>,
    incoming: Mutex<Receiver<Result<Value, String>>>,
    next_id: AtomicU64,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    stopped: AtomicBool,
}

impl AgentProcess {
    pub fn spawn(program: &str, args: &[String], cwd: &Path) -> io::Result<Arc<Self>> {
        #[cfg(target_os = "macos")]
        let path = {
            let inherited = std::env::var_os("PATH").unwrap_or_default();
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            crate::terminal_session::build_macos_path(&inherited, &home, Path::is_dir)
        };
        #[cfg(not(target_os = "macos"))]
        let path = None;
        let mut child = agent_command(program, args, cwd, path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("agent stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("agent stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("agent stderr was not piped"))?;
        let (out_tx, out_rx) = mpsc::sync_channel::<String>(64);
        let (in_tx, in_rx) = mpsc::sync_channel::<Result<Value, String>>(64);
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_BYTES)));

        thread::spawn(move || {
            let mut stdin = stdin;
            while let Ok(line) = out_rx.recv() {
                if stdin.write_all(line.as_bytes()).is_err()
                    || stdin.write_all(b"\n").is_err()
                    || stdin.flush().is_err()
                {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader
                    .by_ref()
                    .take((MAX_FRAME_BYTES + 1) as u64)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) => break,
                    Ok(_) if line.len() > MAX_FRAME_BYTES => {
                        let _ = in_tx.send(Err(format!(
                            "agent message exceeds the {MAX_FRAME_BYTES}-byte limit"
                        )));
                        break;
                    }
                    Ok(_) => {
                        while matches!(line.last(), Some(b'\n' | b'\r')) {
                            line.pop();
                        }
                        if line.is_empty() {
                            continue;
                        }
                        let parsed = serde_json::from_slice(&line)
                            .map_err(|error| format!("invalid agent JSON-RPC message: {error}"));
                        if in_tx.send(parsed).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = in_tx.send(Err(format!("could not read agent output: {error}")));
                        break;
                    }
                }
            }
        });
        let stderr_writer = stderr_tail.clone();
        thread::spawn(move || {
            let mut stderr = stderr;
            let mut buffer = [0; 1024];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if let Ok(mut tail) = stderr_writer.lock() {
                            for byte in &buffer[..count] {
                                if tail.len() == STDERR_TAIL_BYTES {
                                    tail.pop_front();
                                }
                                tail.push_back(*byte);
                            }
                        }
                    }
                }
            }
        });

        Ok(Arc::new(Self {
            child: Mutex::new(child),
            outgoing: out_tx,
            incoming: Mutex::new(in_rx),
            next_id: AtomicU64::new(1),
            stderr: stderr_tail,
            stopped: AtomicBool::new(false),
        }))
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn send(&self, message: Value) -> Result<(), String> {
        let line = serde_json::to_string(&message).map_err(|error| error.to_string())?;
        if line.len() > MAX_FRAME_BYTES {
            return Err(format!(
                "outgoing agent message exceeds the {MAX_FRAME_BYTES}-byte limit"
            ));
        }
        self.outgoing
            .try_send(line)
            .map_err(|error| format!("could not send message to agent: {error}"))
    }

    pub fn try_recv(&self) -> Result<Option<Value>, String> {
        let receiver = self
            .incoming
            .lock()
            .map_err(|_| "agent message queue was poisoned".to_string())?;
        match receiver.try_recv() {
            Ok(message) => message.map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Ok(None),
        }
    }

    pub fn stderr_tail(&self) -> String {
        self.stderr
            .lock()
            .map(|tail| {
                String::from_utf8_lossy(&tail.iter().copied().collect::<Vec<_>>())
                    .trim()
                    .to_string()
            })
            .unwrap_or_default()
    }

    pub fn try_wait(&self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child
            .lock()
            .map_err(|_| io::Error::other("agent process lock was poisoned"))?
            .try_wait()
    }

    pub fn stop(&self) {
        if self.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn agent_command(program: &str, args: &[String], cwd: &Path, path: Option<OsString>) -> Command {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
pub fn parse_frame(frame: &[u8]) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(frame)
}

#[cfg(test)]
pub fn encode_frame(message: &Value) -> Result<Vec<u8>, serde_json::Error> {
    let mut frame = serde_json::to_vec(message)?;
    frame.push(b'\n');
    Ok(frame)
}

pub fn rpc_request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

pub fn rpc_notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","method":method,"params":params})
}

pub fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

pub fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

pub fn initialize_params() -> Value {
    json!({
        "protocolVersion": 1,
        "clientInfo": {"name": "Rista", "version": env!("CARGO_PKG_VERSION")},
        "clientCapabilities": {
            "fs": {"readTextFile": true, "writeTextFile": true},
            "terminal": false
        }
    })
}

pub fn session_new_params(cwd: &Path) -> Value {
    json!({"cwd":cwd,"mcpServers":[]})
}

pub fn prompt_content(text: &str, paths: &[PathBuf]) -> Vec<Value> {
    let mut content = vec![json!({"type":"text","text":text})];
    for path in paths {
        let Ok(url) = url::Url::from_file_path(path) else {
            continue;
        };
        content.push(json!({
            "type": "resource_link",
            "uri": url.as_str(),
            "name": path.file_name().unwrap_or_default().to_string_lossy(),
            "mimeType": mime_type(path)
        }));
    }
    content
}

fn mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("md" | "markdown") => "text/markdown",
        Some("txt") => "text/plain",
        Some("json") => "application/json",
        Some("rs") => "text/x-rust",
        Some("py") => "text/x-python",
        _ => "text/plain",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallContent {
    pub text: String,
    pub diff: Option<(String, String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub title: String,
    pub status: String,
    pub content: Vec<ToolCallContent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    pub content: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptItem {
    User(String),
    Message(String),
    Thought(String),
    Tool(String),
    Plan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateState {
    pub transcript: Vec<TranscriptItem>,
    pub tools: Vec<ToolCall>,
    pub plan: Vec<PlanEntry>,
}

pub fn reduce_update(state: &mut UpdateState, update: &Value) {
    let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
        return;
    };
    match kind {
        "agent_message_chunk" => {
            let text = content_text(update.get("content"));
            if text.is_empty() {
                return;
            }
            match state.transcript.last_mut() {
                Some(TranscriptItem::Message(existing)) => existing.push_str(&text),
                _ => state.transcript.push(TranscriptItem::Message(text)),
            }
        }
        "agent_thought_chunk" => {
            let text = content_text(update.get("content"));
            if text.is_empty() {
                return;
            }
            match state.transcript.last_mut() {
                Some(TranscriptItem::Thought(existing)) => existing.push_str(&text),
                _ => state.transcript.push(TranscriptItem::Thought(text)),
            }
        }
        "tool_call" | "tool_call_update" => {
            let id_field = "toolCallId";
            let Some(id) = update.get(id_field).and_then(Value::as_str) else {
                return;
            };
            let index = state.tools.iter().position(|call| call.id == id);
            let new_call = ToolCall {
                id: id.to_string(),
                title: update
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Tool")
                    .to_string(),
                status: update
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("pending")
                    .to_string(),
                content: tool_contents(update.get("content")),
            };
            if let Some(index) = index {
                let existing = &mut state.tools[index];
                if update.get("title").is_some() {
                    existing.title = new_call.title;
                }
                if update.get("status").is_some() {
                    existing.status = new_call.status;
                }
                if update.get("content").is_some() {
                    existing.content = new_call.content;
                }
            } else {
                state.tools.push(new_call);
                state.transcript.push(TranscriptItem::Tool(id.to_string()));
            }
        }
        "plan" => {
            state.plan = update
                .get("entries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    Some(PlanEntry {
                        content: entry.get("content")?.as_str()?.to_string(),
                        status: entry
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("pending")
                            .to_string(),
                    })
                })
                .collect();
            if !state
                .transcript
                .iter()
                .any(|item| matches!(item, TranscriptItem::Plan))
            {
                state.transcript.push(TranscriptItem::Plan);
            }
        }
        _ => {}
    }
}

fn content_text(content: Option<&Value>) -> String {
    content
        .and_then(|content| content.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn tool_contents(content: Option<&Value>) -> Vec<ToolCallContent> {
    content
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|item| {
            let diff = item
                .get("diff")
                .or_else(|| (item.get("type")?.as_str()? == "diff").then_some(item));
            if let Some(diff) = diff {
                ToolCallContent {
                    text: String::new(),
                    diff: Some((
                        diff.get("path")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        diff.get("oldText")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        diff.get("newText")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    )),
                }
            } else {
                ToolCallContent {
                    text: item
                        .get("content")
                        .and_then(|content| content.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    diff: None,
                }
            }
        })
        .collect()
}

pub fn guard_vault_path(root: &Path, candidate: &Path) -> io::Result<PathBuf> {
    if !candidate.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "agent file paths must be absolute",
        ));
    }
    let root = root.canonicalize()?;
    let resolved = match candidate.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = candidate.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::PermissionDenied, "file path has no parent")
            })?;
            let name = candidate.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::PermissionDenied, "file path has no filename")
            })?;
            parent.canonicalize()?.join(name)
        }
        Err(error) => return Err(error),
    };
    if !resolved.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "agent file path escapes the vault",
        ));
    }
    Ok(resolved)
}

pub fn read_text_file(
    root: &Path,
    candidate: &Path,
    line: Option<usize>,
    limit: Option<usize>,
    open_buffers: &HashMap<PathBuf, String>,
) -> io::Result<String> {
    let path = guard_vault_path(root, candidate)?;
    let text = if let Some(text) = open_buffers.get(&path) {
        text.clone()
    } else {
        std::fs::read_to_string(path)?
    };
    Ok(slice_lines(&text, line, limit))
}

pub fn slice_lines(text: &str, line: Option<usize>, limit: Option<usize>) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    let start = line.unwrap_or(1).max(1).saturating_sub(1).min(lines.len());
    let end = limit
        .map(|limit| start.saturating_add(limit))
        .unwrap_or(lines.len())
        .min(lines.len());
    let mut result = lines[start..end].join("\n");
    if end == lines.len() && text.ends_with('\n') && !result.is_empty() {
        result.push('\n');
    }
    result
}

pub fn simple_diff(path: &str, old_text: &str, new_text: &str) -> String {
    let old = old_text.lines().collect::<Vec<_>>();
    let new = new_text.lines().collect::<Vec<_>>();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    if prefix == old.len() && prefix == new.len() {
        return String::new();
    }
    let old_end = old.len().saturating_sub(suffix);
    let new_end = new.len().saturating_sub(suffix);
    let mut diff = format!(
        "--- {path}\n+++ {path}\n@@ -{},{} +{},{} @@\n",
        prefix + 1,
        old_end.saturating_sub(prefix),
        prefix + 1,
        new_end.saturating_sub(prefix)
    );
    for line in &old[prefix..old_end] {
        diff.push('-');
        diff.push_str(line);
        diff.push('\n');
    }
    for line in &new[prefix..new_end] {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "rista-agent-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create temporary test directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn frames_round_trip_as_json_lines() {
        let message = rpc_request(7, "session/prompt", json!({"text":"hello"}));
        let frame = encode_frame(&message).expect("encode frame");
        assert!(frame.ends_with(b"\n"));
        assert_eq!(parse_frame(&frame).expect("parse frame"), message);
    }

    #[test]
    fn builds_acp_initialization_session_and_resource_link_prompts() {
        let temp = TempDir::new();
        let path = temp.0.join("note.md");
        let init = initialize_params();
        assert_eq!(init["protocolVersion"], 1);
        assert_eq!(init["clientCapabilities"]["fs"]["readTextFile"], true);
        assert_eq!(init["clientCapabilities"]["terminal"], false);
        assert_eq!(
            session_new_params(&temp.0)["cwd"],
            temp.0.to_string_lossy().as_ref()
        );
        let prompt = prompt_content("Review this", std::slice::from_ref(&path));
        assert_eq!(prompt[0]["type"], "text");
        assert_eq!(prompt[1]["type"], "resource_link");
        assert!(prompt[1]["uri"].as_str().unwrap().starts_with("file:"));
    }

    #[test]
    fn discovers_config_and_legacy_models_without_inventing_choices() {
        let config = model_state_from_session(&json!({
            "configOptions": [
                {"id":"reasoning","type":"select","options":[{"name":"Fast","value":"fast"}]},
                {
                    "id":"model","category":"model","type":"select","currentValue":"vendor/a",
                    "options":[
                        {"name":"Vendor A","value":"vendor/a"},
                        {"groupName":"Other","options":[
                            {"name":"Vendor B","value":"vendor/b"},
                            {"name":"Duplicate vendor A","value":"vendor/a"},
                            {"name":"Malformed","id":"missing-value"},
                            {"name":"Vendor C","value":"vendor/c"}
                        ]}
                    ]
                }
            ]
        }));
        assert_eq!(config.config_id.as_deref(), Some("model"));
        assert_eq!(config.selected.as_deref(), Some("vendor/a"));
        assert_eq!(
            config
                .choices
                .iter()
                .map(|choice| choice.value.as_str())
                .collect::<Vec<_>>(),
            ["vendor/a", "vendor/b", "vendor/c"]
        );

        let legacy = model_state_from_session(&json!({
            "models": {
                "availableModels": [
                    {"modelId":"legacy/a","name":"Legacy A"},
                    {"modelId":"legacy/a","name":"Duplicate legacy A"},
                    {"name":"Malformed"}
                ],
                "currentModelId":"legacy/a"
            }
        }));
        assert_eq!(legacy.config_id, None);
        assert_eq!(legacy.choices.len(), 1);
        assert_eq!(legacy.choices[0].value, "legacy/a");
        assert_eq!(legacy.choices[0].name, "Legacy A");
        assert_eq!(legacy.selected.as_deref(), Some("legacy/a"));
        assert_eq!(
            model_state_from_session(&json!({"models":{"other":true}})),
            ModelState::default()
        );
        assert_eq!(
            model_state_from_session(&json!({"configOptions":[{"id":"unknown","type":"select"}]})),
            ModelState::default()
        );
    }

    #[test]
    fn grok_model_filter_requires_an_advertised_xai_selection() {
        let state = model_state_from_session(&json!({
            "models":{
                "availableModels":[
                    {"modelId":"openai/gpt-4o","name":"Other provider"},
                    {"modelId":"xai/grok-3","name":"Grok 3"}
                ],
                "currentModelId":"openai/gpt-4o"
            }
        }));
        let choices = allowed_model_choices(&state, true);
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].value, "xai/grok-3");
        assert!(!choices.iter().any(|choice| choice.value == "openai/gpt-4o"));
        assert!(allowed_model_choices(&ModelState::default(), true).is_empty());
        assert!(!has_usable_model(&state, true));
        assert!(!has_usable_model(&ModelState::default(), true));
        let no_xai = model_state_from_session(&json!({
            "models":{"availableModels":[{"modelId":"openai/gpt-4o","name":"Other"}],
                "currentModelId":"openai/gpt-4o"}
        }));
        assert!(allowed_model_choices(&no_xai, true).is_empty());
        assert!(!has_usable_model(&no_xai, true));
        assert!(has_usable_model(&ModelState::default(), false));
        let mut selected_xai = state.clone();
        selected_xai.selected = Some("xai/grok-3".into());
        assert!(has_usable_model(&selected_xai, true));
        assert_eq!(allowed_model_choices(&state, false).len(), 2);
    }

    #[test]
    fn xai_filter_precedes_display_cap_and_keeps_full_selection_validation() {
        let many: Vec<_> = (0..70)
            .map(
                |index| json!({"modelId":format!("other/{index}"),"name":format!("Other {index}")}),
            )
            .chain(std::iter::once(json!({"modelId":"xai/last","name":"Grok"})))
            .collect();
        let state = model_state_from_session(&json!({
            "models":{"availableModels":many,"currentModelId":"xai/last"}
        }));
        assert_eq!(allowed_model_choices(&state, true).len(), 1);
        assert!(has_usable_model(&state, true));
        let grouped: Vec<_> = (0..70)
            .map(|index| json!({"name":format!("Other {index}"),"value":format!("other/{index}")}))
            .chain(std::iter::once(json!({"name":"Grok","value":"xai/last"})))
            .collect();
        let config = model_state_from_config_options(&[json!({
            "id":"model","type":"select","category":"model","currentValue":"xai/last",
            "options":[{"name":"Providers","options":grouped}]
        })])
        .unwrap();
        assert_eq!(allowed_model_choices(&config, true).len(), 1);
        let many_xai = ModelState {
            choices: (0..70)
                .map(|index| ModelChoice {
                    value: format!("xai/{index}"),
                    name: format!("Grok {index}"),
                })
                .collect(),
            selected: Some("xai/69".into()),
            ..ModelState::default()
        };
        assert_eq!(
            allowed_model_choices(&many_xai, true).len(),
            MAX_MODEL_CHOICES
        );
        assert!(has_usable_model(&many_xai, true));
        assert_eq!(selected_model_name(&many_xai, true), Some("Grok 69"));
    }

    #[test]
    fn config_option_update_uses_documented_session_update_envelope() {
        let params = json!({
            "sessionId":"active",
            "update":{"sessionUpdate":"config_option_update","configOptions":[
                {"id":"model","type":"select","category":"model","currentValue":"xai/grok",
                 "options":[{"name":"Grok","value":"xai/grok"}]}
            ]}
        });
        let update = session_update(&params, "active").unwrap();
        let options = config_options_update(update).unwrap();
        let replacement = model_state_from_config_options(options).unwrap();
        assert_eq!(replacement.selected.as_deref(), Some("xai/grok"));
        assert!(session_update(&params, "other").is_none());
        let stream = json!({"sessionId":"active","update":{
            "sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello"}
        }});
        let stream_update = session_update(&stream, "active").unwrap();
        assert!(config_options_update(stream_update).is_none());
        let mut stream_state = UpdateState::default();
        reduce_update(&mut stream_state, stream_update);
        assert_eq!(
            stream_state.transcript,
            vec![TranscriptItem::Message("hello".into())]
        );
    }

    #[test]
    fn model_switch_requests_and_selection_state_are_safe() {
        assert_eq!(
            model_switch_request(4, "session", Some("model"), "xai/grok-3"),
            rpc_request(
                4,
                "session/set_config_option",
                json!({"sessionId":"session","configId":"model","value":"xai/grok-3"})
            )
        );
        assert_eq!(
            model_switch_request(5, "session", None, "legacy/model"),
            rpc_request(
                5,
                "session/set_model",
                json!({"sessionId":"session","modelId":"legacy/model"})
            )
        );
        let mut selection = ModelSelection {
            state: ModelState {
                config_id: Some("model".into()),
                choices: Vec::new(),
                selected: Some("previous".into()),
            },
            pending: None,
        };
        assert!(selection.begin(8, "requested".into()));
        assert!(!selection.begin(9, "other".into()));
        assert!(!selection.complete(9, Some("wrong".into())));
        assert!(selection.fail(8));
        assert_eq!(selection.state.selected.as_deref(), Some("previous"));
        assert!(selection.begin(10, "requested".into()));
        assert_eq!(selection.state.selected.as_deref(), Some("previous"));
        assert!(selection.complete(10, Some("agent-selected".into())));
        assert_eq!(selection.state.selected.as_deref(), Some("agent-selected"));
        assert!(selection.begin(11, "requested".into()));
        selection.reset();
        assert!(selection.pending.is_none());
    }

    #[test]
    fn prompt_gate_blocks_unready_switching_and_missing_models() {
        let can_send = |ready, pending, model| {
            can_prompt(ready, true, true, false, pending, model, "keep this prompt")
        };
        assert!(can_send(true, false, true));
        assert!(!can_send(false, false, true));
        assert!(!can_send(true, true, true));
        assert!(!can_send(true, false, false));
        assert!(!can_prompt(true, true, true, false, false, true, " \n"));
    }

    #[test]
    fn agent_selection_is_blocked_while_a_process_exists() {
        assert!(can_select_agent(false));
        assert!(!can_select_agent(true));
    }

    #[test]
    fn config_option_response_supplies_the_selected_model() {
        let mut old_state = model_state_from_session(&json!({
            "configOptions":[{"id":"model","type":"select","category":"model","currentValue":"old/model",
                "options":[{"name":"Old model","value":"old/model"},{"name":"Removed model","value":"removed/model"}]}]
        }));
        assert_eq!(old_state.choices.len(), 2);
        let result = json!({
            "configOptions": [
                {"id":"other","currentValue":"unrelated"},
                {"id":"model","category":"model","type":"select","currentValue":"xai/grok-3",
                 "options":[{"name":"Grok 3","value":"xai/grok-3"}]}
            ]
        });
        old_state = model_state_from_session(&result);
        assert_eq!(old_state.choices.len(), 1);
        assert_eq!(old_state.choices[0].value, "xai/grok-3");
        assert!(!old_state
            .choices
            .iter()
            .any(|choice| choice.value == "removed/model"));
        assert_eq!(
            config_result_model_value(&result, "model").as_deref(),
            Some("xai/grok-3")
        );
        assert_eq!(
            config_result_model_value(&result, "other").as_deref(),
            Some("unrelated")
        );
        assert_eq!(config_result_model_value(&result, "missing"), None);
    }

    #[cfg(unix)]
    #[test]
    fn agent_command_keeps_literal_args_and_explicit_path() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new();
        let executable = temp.0.join("agent executable with spaces");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$PATH\" \"$1\" \"$2\"\n",
        )
        .expect("write executable");
        let mut permissions = fs::metadata(&executable).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("make executable");
        let child_path = OsString::from("/tmp/path with spaces");
        let output = agent_command(
            executable.to_str().expect("UTF-8 test path"),
            &["argument with spaces".into(), "$(not-shell)".into()],
            &temp.0,
            Some(child_path.clone()),
        )
        .output()
        .expect("spawn executable");
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 output"),
            format!(
                "{}\nargument with spaces\n$(not-shell)\n",
                child_path.to_string_lossy()
            )
        );
    }

    #[test]
    fn reducer_appends_chunks_merges_tool_updates_and_ignores_unknown_updates() {
        let mut state = UpdateState::default();
        reduce_update(
            &mut state,
            &json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello "}}),
        );
        reduce_update(
            &mut state,
            &json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"world"}}),
        );
        reduce_update(
            &mut state,
            &json!({"sessionUpdate":"tool_call","toolCallId":"t1","title":"Read","status":"in_progress"}),
        );
        reduce_update(
            &mut state,
            &json!({"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"done"}}]}),
        );
        reduce_update(&mut state, &json!({"sessionUpdate":"future_update"}));

        assert_eq!(
            state.transcript,
            vec![
                TranscriptItem::Message("hello world".into()),
                TranscriptItem::Tool("t1".into())
            ]
        );
        assert_eq!(state.tools.len(), 1);
        assert_eq!(state.tools[0].title, "Read");
        assert_eq!(state.tools[0].status, "completed");
        assert_eq!(state.tools[0].content[0].text, "done");
    }

    #[test]
    fn reducer_preserves_protocol_diff_content() {
        let mut state = UpdateState::default();
        reduce_update(
            &mut state,
            &json!({
                "sessionUpdate":"tool_call",
                "toolCallId":"edit-1",
                "content":[{
                    "type":"diff",
                    "path":"note.md",
                    "oldText":"before\n",
                    "newText":"after\n"
                }]
            }),
        );
        assert_eq!(
            state.tools[0].content[0].diff,
            Some(("note.md".into(), "before\n".into(), "after\n".into()))
        );
    }

    #[test]
    fn guards_vault_paths_and_prefers_open_buffer_text() {
        let temp = TempDir::new();
        let root = temp.0.join("vault");
        fs::create_dir_all(&root).expect("create vault");
        let note = root.join("note.md");
        fs::write(&note, "disk\ntext\n").expect("write note");
        let canonical_note = note.canonicalize().expect("canonicalize note");
        let buffers = HashMap::from([(canonical_note, "unsaved\nbuffer\n".to_string())]);

        assert_eq!(
            read_text_file(&root, &note, Some(2), Some(1), &buffers).expect("read buffer"),
            "buffer\n"
        );
        assert_eq!(
            guard_vault_path(&root, &temp.0.join("outside.md"))
                .expect_err("reject outside path")
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn guards_symlink_escapes_and_slices_one_based_ranges() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let temp = TempDir::new();
            let root = temp.0.join("vault");
            fs::create_dir_all(&root).expect("create vault");
            let outside = temp.0.join("outside.md");
            fs::write(&outside, "secret").expect("write outside file");
            let link = root.join("linked.md");
            symlink(&outside, &link).expect("create escape symlink");
            assert_eq!(
                guard_vault_path(&root, &link)
                    .expect_err("reject symlink escape")
                    .kind(),
                io::ErrorKind::PermissionDenied
            );
        }
        assert_eq!(slice_lines("a\nb\nc\n", Some(2), Some(1)), "b");
        assert_eq!(slice_lines("a\nb\nc\n", Some(4), None), "");
    }

    #[test]
    fn computes_a_small_reviewable_diff() {
        assert_eq!(
            simple_diff("note.md", "before\nkeep\n", "after\nkeep\n"),
            "--- note.md\n+++ note.md\n@@ -1,1 +1,1 @@\n-before\n+after\n"
        );
    }
}
