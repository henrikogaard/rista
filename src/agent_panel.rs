use super::*;
use crate::agent::{self, AgentProcess, PlanEntry, TranscriptItem, UpdateState};
use crate::extensions::{self, Agent};
use gpui_kit::assets;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{self, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
struct AgentChoice {
    manifest_id: Option<String>,
    agent: Agent,
}

fn normalize_context_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn merge_active_context(
    context: &mut Vec<PathBuf>,
    previous: Option<&Path>,
    next: Option<&Path>,
    explicit: &HashSet<PathBuf>,
    dismissed: &HashSet<PathBuf>,
) {
    if let Some(previous) = previous {
        if !explicit.contains(previous) {
            context.retain(|path| path != previous);
        }
    }
    if let Some(next) = next {
        if !dismissed.contains(next) && !context.iter().any(|path| path == next) {
            context.insert(0, next.to_path_buf());
        }
    }
}

#[derive(Clone)]
struct PermissionOption {
    id: String,
    name: String,
    kind: String,
}

struct PendingPermission {
    id: Value,
    title: String,
    options: Vec<PermissionOption>,
}

struct PendingWrite {
    id: Value,
    path: PathBuf,
    before: String,
    after: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AgentStatus {
    Idle,
    Connecting,
    Ready,
    Working,
    Stopping,
    Error,
}

pub(super) struct AgentPanel {
    workspace: WeakEntity<Workspace>,
    composer: Entity<TextareaState>,
    _composer_sub: Subscription,
    focus_handle: FocusHandle,
    selected: Option<AgentChoice>,
    picker_open: bool,
    context: Vec<PathBuf>,
    explicit_context: HashSet<PathBuf>,
    dismissed_context: HashSet<PathBuf>,
    active_context: Option<PathBuf>,
    conversation_root: Option<PathBuf>,
    process: Option<Arc<AgentProcess>>,
    session_id: Option<String>,
    initialize_id: Option<u64>,
    session_id_request: Option<u64>,
    prompt_id: Option<u64>,
    stopping: bool,
    status: AgentStatus,
    error: Option<String>,
    update: UpdateState,
    pending_permissions: Vec<PendingPermission>,
    pending_writes: Vec<PendingWrite>,
    expanded_tools: HashSet<String>,
    thoughts_expanded: bool,
}

impl AgentPanel {
    pub(super) fn new(
        workspace: WeakEntity<Workspace>,
        language: crate::settings::Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .submit_on_enter(true)
                .placeholder(language.text("Ask the agent…", "Spør agenten…"))
        });
        let composer_sub = cx.subscribe(&composer, |_, _, _: &input::InputEvent, cx| {
            cx.notify();
        });
        Self {
            workspace,
            composer,
            _composer_sub: composer_sub,
            focus_handle: cx.focus_handle(),
            selected: None,
            picker_open: false,
            context: Vec::new(),
            explicit_context: HashSet::new(),
            dismissed_context: HashSet::new(),
            active_context: None,
            conversation_root: None,
            process: None,
            session_id: None,
            initialize_id: None,
            session_id_request: None,
            prompt_id: None,
            stopping: false,
            status: AgentStatus::Idle,
            error: None,
            update: UpdateState::default(),
            pending_permissions: Vec::new(),
            pending_writes: Vec::new(),
            expanded_tools: HashSet::new(),
            thoughts_expanded: false,
        }
    }

    fn language(&self, cx: &App) -> crate::settings::Language {
        self.workspace
            .upgrade()
            .map(|workspace| workspace.read(cx).settings.language)
            .unwrap_or_default()
    }

    fn tr(&self, cx: &App, en: &'static str, nb: &'static str) -> &'static str {
        self.language(cx).text(en, nb)
    }

    pub(super) fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
    }

    pub(super) fn sync_active_path(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let next = path.map(|path| normalize_context_path(&path));
        if self.active_context == next {
            return;
        }
        merge_active_context(
            &mut self.context,
            self.active_context.as_deref(),
            next.as_deref(),
            &self.explicit_context,
            &self.dismissed_context,
        );
        self.active_context = next;
        cx.notify();
    }

    pub(super) fn add_context(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = normalize_context_path(&path);
        if self.is_vault_path(&path, cx) {
            self.dismissed_context.remove(&path);
            self.explicit_context.insert(path.clone());
            if !self.context.contains(&path) {
                self.context.push(path);
                cx.notify();
            }
        }
    }

    fn remove_context(&mut self, index: usize) {
        if index >= self.context.len() {
            return;
        }
        let path = self.context.remove(index);
        self.explicit_context.remove(&path);
        if self.active_context.as_ref() == Some(&path) {
            self.dismissed_context.insert(path);
        }
    }

    pub(super) fn vault_changed(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
        self.context.clear();
        self.explicit_context.clear();
        self.dismissed_context.clear();
        self.active_context = None;
        self.selected = None;
        cx.notify();
    }

    fn is_vault_path(&self, path: &Path, cx: &App) -> bool {
        self.workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).vault.read(cx).root.clone())
            .is_some_and(|root| agent::guard_vault_path(&root, path).is_ok())
    }

    fn choices(&self, cx: &App) -> (Vec<AgentChoice>, Vec<String>) {
        let root = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).vault.read(cx).root.clone());
        let registry = extensions::Registry::load(&extensions::user_dir(), root.as_deref());
        let choices = extensions::registered_agents(registry.plugins)
            .into_iter()
            .map(|(manifest_id, agent)| AgentChoice { manifest_id, agent })
            .collect();
        let errors = registry
            .errors
            .iter()
            .map(|error| error.text(self.language(cx)))
            .collect();
        (choices, errors)
    }

    fn confirm_launch(
        &mut self,
        choice: AgentChoice,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.language(cx);
        let name = choice.agent.name.text(language).to_string();
        let args = serde_json::to_string(&choice.agent.args).unwrap_or_else(|_| "[]".into());
        let program = choice.agent.program.clone();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            let choice = choice.clone();
            let root = root.clone();
            let program = program.clone();
            let args = args.clone();
            dialog
                .title(format!("{} {name}?", language.text("Start", "Start")))
                .description(format!(
                    "{}\n{}\n{}\n{}\n{}\n{}\n\n{}",
                    language.text("Executable:", "Kjørbar fil:"),
                    program,
                    language.text("Arguments:", "Argumenter:"),
                    args,
                    language.text("Working directory:", "Arbeidsmappe:"),
                    root.display(),
                    language.text(
                        "Runs with your user permissions and is not sandboxed. It can access files and the network. Approval applies only to file writes requested through ACP; the agent and its tools may change files directly.",
                        "Kjører med brukerrettighetene dine og er ikke sandkasseisolert. Den kan få tilgang til filer og nettverk. Godkjenning gjelder bare filskriving via ACP; agenten og verktøyene kan endre filer direkte."
                    )
                ))
                .show_cancel(true)
                .ok_text(language.text("Start agent", "Start agent"))
                .on_ok(move |_, window, cx| {
                    window.close_dialog(cx);
                    view.update(cx, |panel, cx| {
                        panel.start(choice.clone(), root.clone(), window, cx)
                    });
                    false
                })
        });
    }

    fn start(
        &mut self,
        choice: AgentChoice,
        root: PathBuf,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop(cx);
        self.selected = Some(choice.clone());
        self.conversation_root = Some(root.clone());
        self.session_id = None;
        self.stopping = false;
        self.update = UpdateState::default();
        self.pending_permissions.clear();
        self.pending_writes.clear();
        self.error = None;
        self.status = AgentStatus::Connecting;
        let process = match AgentProcess::spawn(&choice.agent.program, &choice.agent.args, &root) {
            Ok(process) => process,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && choice.agent.id == extensions::BUILTIN_VIBE_AGENT_ID =>
            {
                let hint = self.tr(
                    cx,
                    "Install Vibe with `uv tool install mistral-vibe`, then run `vibe` once in Terminal to sign in and configure it.",
                    "Installer Vibe med `uv tool install mistral-vibe`, og kjør deretter `vibe` én gang i Terminal for å logge inn og konfigurere.",
                );
                self.set_process_error(format!("{hint}\n{error}"), cx);
                return;
            }
            Err(error) => {
                self.set_process_error(error.to_string(), cx);
                return;
            }
        };
        let id = process.next_id();
        self.initialize_id = Some(id);
        if let Err(error) = process.send(agent::rpc_request(
            id,
            "initialize",
            agent::initialize_params(),
        )) {
            self.set_process_error(error, cx);
            process.stop();
            return;
        }
        self.process = Some(process.clone());
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| loop {
            smol::Timer::after(Duration::from_millis(30)).await;
            let keep_polling = entity
                .update(&mut *cx, |panel, cx| panel.poll(&process, cx))
                .unwrap_or(false);
            if !keep_polling {
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn poll(&mut self, process: &Arc<AgentProcess>, cx: &mut Context<Self>) -> bool {
        if !self
            .process
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, process))
        {
            return false;
        }
        for _ in 0..64 {
            match process.try_recv() {
                Ok(Some(message)) => self.handle_message(message, cx),
                Ok(None) => break,
                Err(error) => {
                    self.set_process_error(error, cx);
                    let process = process.clone();
                    std::thread::spawn(move || process.stop());
                    return false;
                }
            }
        }
        match process.try_wait() {
            Ok(Some(status)) => {
                if self.stopping {
                    self.finish_stop(cx);
                } else if self.status != AgentStatus::Idle && self.status != AgentStatus::Error {
                    let tail = process.stderr_tail();
                    self.set_error(
                        if tail.is_empty() {
                            let text = self.tr(cx, "Agent exited with", "Agenten avsluttet med");
                            format!("{text} {status}")
                        } else {
                            tail
                        },
                        cx,
                    );
                }
                false
            }
            Err(error) => {
                self.set_process_error(error.to_string(), cx);
                let process = process.clone();
                std::thread::spawn(move || process.stop());
                false
            }
            Ok(None) => true,
        }
    }

    fn handle_message(&mut self, message: Value, cx: &mut Context<Self>) {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if method == "session/update" {
                if let Some(update) = message
                    .get("params")
                    .and_then(|params| params.get("update"))
                {
                    agent::reduce_update(&mut self.update, update);
                    cx.notify();
                }
                return;
            }
            let Some(id) = message.get("id").cloned() else {
                return;
            };
            self.handle_request(
                &id,
                method,
                message.get("params").unwrap_or(&Value::Null),
                cx,
            );
            return;
        }

        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return;
        };
        if self.initialize_id == Some(id) {
            self.initialize_id = None;
            if let Some(error) = message.get("error") {
                self.set_protocol_error(error, cx);
                return;
            }
            let version = message
                .get("result")
                .and_then(|result| result.get("protocolVersion"))
                .and_then(Value::as_u64);
            if version != Some(1) {
                self.set_error(
                    self.tr(
                        cx,
                        "The agent does not support ACP protocol version 1.",
                        "Agenten støtter ikke ACP-protokollversjon 1.",
                    )
                    .to_string(),
                    cx,
                );
                if let Some(process) = self.process.clone() {
                    std::thread::spawn(move || process.stop());
                }
                return;
            }
            self.create_session(cx);
        } else if self.session_id_request == Some(id) {
            self.session_id_request = None;
            if let Some(error) = message.get("error") {
                self.set_protocol_error(error, cx);
                return;
            }
            self.session_id = message
                .get("result")
                .and_then(|result| result.get("sessionId"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            if self.session_id.is_some() {
                self.status = AgentStatus::Ready;
                self.error = None;
                cx.notify();
            } else {
                self.set_error(
                    self.tr(
                        cx,
                        "The agent did not return a session ID.",
                        "Agenten returnerte ingen økt-ID.",
                    )
                    .to_string(),
                    cx,
                );
            }
        } else if self.prompt_id == Some(id) {
            self.prompt_id = None;
            if self.stopping {
                self.finish_cancel(cx);
            } else if let Some(error) = message.get("error") {
                self.set_protocol_error(error, cx);
            } else {
                self.status = AgentStatus::Ready;
                cx.notify();
            }
        }
    }

    fn create_session(&mut self, cx: &mut Context<Self>) {
        let (Some(process), Some(root)) = (&self.process, &self.conversation_root) else {
            return;
        };
        let id = process.next_id();
        self.session_id_request = Some(id);
        if let Err(error) = process.send(agent::rpc_request(
            id,
            "session/new",
            agent::session_new_params(root),
        )) {
            self.set_process_error(error, cx);
        }
    }

    fn handle_request(&mut self, id: &Value, method: &str, params: &Value, cx: &mut Context<Self>) {
        match method {
            "session/request_permission" => {
                if self.stopping {
                    self.respond_result(id, json!({"outcome":{"outcome":"cancelled"}}));
                    return;
                }
                let options = params
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|option| {
                        Some(PermissionOption {
                            id: option.get("optionId")?.as_str()?.to_string(),
                            name: option.get("name")?.as_str()?.to_string(),
                            kind: option.get("kind")?.as_str()?.to_string(),
                        })
                    })
                    .collect();
                let title = params
                    .get("toolCall")
                    .and_then(|tool| tool.get("title"))
                    .and_then(Value::as_str)
                    .or_else(|| {
                        let id = params
                            .get("toolCall")
                            .and_then(|tool| tool.get("toolCallId"))
                            .and_then(Value::as_str)?;
                        Some(
                            self.update
                                .tools
                                .iter()
                                .find(|tool| tool.id == id && !tool.title.is_empty())
                                .map_or(id, |tool| tool.title.as_str()),
                        )
                    })
                    .unwrap_or(self.tr(
                        cx,
                        "Agent requests permission",
                        "Agenten ber om tillatelse",
                    ))
                    .to_string();
                self.pending_permissions.push(PendingPermission {
                    id: id.clone(),
                    title,
                    options,
                });
                self.status = AgentStatus::Working;
                cx.notify();
            }
            "fs/read_text_file" => {
                let Some(path) = params
                    .get("path")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                else {
                    self.respond_error(id, -32602, "Missing file path");
                    return;
                };
                let start = params
                    .get("line")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize);
                let limit = params
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize);
                match self.read_file(&path, start, limit, cx) {
                    Ok(content) => self.respond_result(id, json!({"content":content})),
                    Err(error) => self.respond_error(id, -32000, &error),
                }
            }
            "fs/write_text_file" => {
                if self.stopping {
                    self.respond_error(id, -32800, "Request cancelled");
                    return;
                }
                let Some(path) = params
                    .get("path")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                else {
                    self.respond_error(id, -32602, "Missing file path");
                    return;
                };
                let Some(content) = params.get("content").and_then(Value::as_str) else {
                    self.respond_error(id, -32602, "Missing file content");
                    return;
                };
                match self.propose_write(id.clone(), path, content.to_string(), cx) {
                    Ok(()) => cx.notify(),
                    Err(error) => self.respond_error(id, -32000, &error),
                }
            }
            _ => self.respond_error(id, -32601, "Method not found"),
        }
    }

    fn read_file(
        &self,
        path: &Path,
        line: Option<usize>,
        limit: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let workspace = self.workspace.upgrade().ok_or("Workspace is unavailable")?;
        workspace.update(cx, |workspace, cx| {
            let root = workspace
                .vault
                .read(cx)
                .root
                .clone()
                .ok_or_else(|| "No vault is open".to_string())?;
            let guarded =
                agent::guard_vault_path(&root, path).map_err(|error| error.to_string())?;
            let open_buffers = workspace
                .docs
                .iter()
                .map(|doc| {
                    let doc = doc.entity.read(cx);
                    let path = doc.path.canonicalize().unwrap_or_else(|_| doc.path.clone());
                    (path, doc.editor.read(cx).value().to_string())
                })
                .collect();
            agent::read_text_file(&root, &guarded, line, limit, &open_buffers)
                .map_err(|error| error.to_string())
        })
    }

    fn propose_write(
        &mut self,
        id: Value,
        path: PathBuf,
        content: String,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let workspace = self.workspace.upgrade().ok_or("Workspace is unavailable")?;
        let (root, guarded, before) = workspace.update(cx, |workspace, cx| {
            let root = workspace
                .vault
                .read(cx)
                .root
                .clone()
                .ok_or_else(|| "No vault is open".to_string())?;
            let guarded =
                agent::guard_vault_path(&root, &path).map_err(|error| error.to_string())?;
            let open = workspace.docs.iter().find_map(|doc| {
                let doc = doc.entity.read(cx);
                let current = doc.path.canonicalize().unwrap_or_else(|_| doc.path.clone());
                (current == guarded).then(|| doc.editor.read(cx).value().to_string())
            });
            let before = match open {
                Some(content) => content,
                None => match std::fs::read_to_string(&guarded) {
                    Ok(content) => content,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                    Err(error) => return Err(error.to_string()),
                },
            };
            Ok::<_, String>((root, guarded, before))
        })?;
        if !guarded.starts_with(&root) {
            return Err("File path is outside the vault".into());
        }
        self.pending_writes.push(PendingWrite {
            id,
            path: guarded,
            before,
            after: content,
        });
        self.status = AgentStatus::Working;
        Ok(())
    }

    fn respond_result(&self, id: &Value, result: Value) {
        if let Some(process) = &self.process {
            let _ = process.send(agent::rpc_result(id.clone(), result));
        }
    }

    fn respond_error(&self, id: &Value, code: i64, message: &str) {
        if let Some(process) = &self.process {
            let _ = process.send(agent::rpc_error(id.clone(), code, message));
        }
    }

    fn set_protocol_error(&mut self, error: &Value, cx: &mut Context<Self>) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(self.tr(cx, "Agent setup failed", "Agentoppsettet mislyktes"));
        if message.to_ascii_lowercase().contains("auth") {
            let hint = self.tr(
                cx,
                "Authentication may be required. Complete it in the agent's own setup, then start a new conversation.",
                "Autentisering kan være nødvendig. Fullfør den i agentens eget oppsett, og start deretter en ny samtale.",
            );
            self.set_error(format!("{hint}\n{message}"), cx);
        } else {
            self.set_process_error(message.to_string(), cx);
        }
    }

    fn set_process_error(&mut self, detail: String, cx: &mut Context<Self>) {
        let prefix = self.tr(cx, "Agent error:", "Agentfeil:");
        self.set_error(format!("{prefix} {detail}"), cx);
    }

    fn set_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.status = AgentStatus::Error;
        self.error = Some(error);
        cx.notify();
    }

    fn send_prompt(&mut self, prompt: String, cx: &mut Context<Self>) {
        let prompt = prompt.trim();
        if prompt.is_empty() || self.status == AgentStatus::Working || self.stopping {
            return;
        }
        let (Some(process), Some(session_id)) = (&self.process, &self.session_id) else {
            return;
        };
        let context = self.context.clone();
        let id = process.next_id();
        if let Err(error) = process.send(agent::rpc_request(
            id,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": agent::prompt_content(prompt, &context)
            }),
        )) {
            self.set_process_error(error, cx);
            return;
        }
        self.prompt_id = Some(id);
        self.update
            .transcript
            .push(TranscriptItem::User(prompt.to_string()));
        self.status = AgentStatus::Working;
        self.error = None;
        cx.notify();
    }

    fn send_from_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.composer.read(cx).value().to_string();
        if prompt.trim().is_empty()
            || self.status == AgentStatus::Working
            || self.stopping
            || self.process.is_none()
            || self.session_id.is_none()
        {
            return;
        }
        self.composer.update(cx, |composer, cx| {
            composer.set_value(String::new(), window, cx)
        });
        self.send_prompt(prompt, cx);
    }

    fn answer_permission(&mut self, request: &Value, option: Option<&str>, cx: &mut Context<Self>) {
        let Some(index) = self
            .pending_permissions
            .iter()
            .position(|pending| &pending.id == request)
        else {
            return;
        };
        let pending = self.pending_permissions.remove(index);
        let outcome = match option {
            Some(option_id) => json!({"outcome":"selected","optionId":option_id}),
            None => json!({"outcome":"cancelled"}),
        };
        self.respond_result(&pending.id, json!({"outcome":outcome}));
        if self.pending_permissions.is_empty() && self.pending_writes.is_empty() {
            self.status = AgentStatus::Working;
        }
        cx.notify();
    }

    fn answer_write(
        &mut self,
        request: &Value,
        accept: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .pending_writes
            .iter()
            .position(|pending| &pending.id == request)
        else {
            return;
        };
        let pending = self.pending_writes.remove(index);
        if !accept {
            self.respond_error(&pending.id, -32800, "Proposed change rejected");
        } else if let Some(workspace) = self.workspace.upgrade() {
            let result = workspace.update(cx, |workspace, cx| {
                workspace.apply_agent_write(
                    &pending.path,
                    &pending.before,
                    &pending.after,
                    window,
                    cx,
                )
            });
            match result {
                Ok(()) => self.respond_result(&pending.id, Value::Null),
                Err(error) => self.respond_error(&pending.id, -32000, &error),
            }
        } else {
            self.respond_error(&pending.id, -32000, "Workspace is unavailable");
        }
        cx.notify();
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(process) = self.process.clone() else {
            self.finish_stop(cx);
            return;
        };
        for pending in self.pending_permissions.drain(..) {
            let _ = process.send(agent::rpc_result(
                pending.id,
                json!({"outcome":{"outcome":"cancelled"}}),
            ));
        }
        for pending in self.pending_writes.drain(..) {
            let _ = process.send(agent::rpc_error(pending.id, -32800, "Request cancelled"));
        }
        for tool in &mut self.update.tools {
            if !matches!(tool.status.as_str(), "completed" | "failed" | "cancelled") {
                tool.status = "cancelled".into();
            }
        }
        if let Some(session_id) = &self.session_id {
            let _ = process.send(agent::rpc_notification(
                "session/cancel",
                json!({"sessionId":session_id}),
            ));
        }

        if self.prompt_id.is_some() {
            self.stopping = true;
            self.status = AgentStatus::Stopping;
            self.error = None;
            let entity = cx.entity().downgrade();
            cx.spawn(async move |_, cx| {
                smol::Timer::after(Duration::from_secs(2)).await;
                let should_kill = entity
                    .update(&mut *cx, |panel, cx| {
                        let same_process = panel
                            .process
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &process));
                        if !same_process {
                            return true;
                        }
                        if panel.stopping {
                            panel.finish_stop(cx);
                            return true;
                        }
                        false
                    })
                    .unwrap_or(true);
                if should_kill {
                    std::thread::spawn(move || process.stop());
                }
            })
            .detach();
        } else {
            self.finish_stop(cx);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(80));
                process.stop();
            });
        }
    }

    fn finish_stop(&mut self, cx: &mut Context<Self>) {
        self.process = None;
        self.session_id = None;
        self.initialize_id = None;
        self.session_id_request = None;
        self.prompt_id = None;
        self.stopping = false;
        self.status = AgentStatus::Idle;
        self.error = None;
        cx.notify();
    }

    fn finish_cancel(&mut self, cx: &mut Context<Self>) {
        self.stopping = false;
        self.status = AgentStatus::Ready;
        self.error = None;
        cx.notify();
    }

    fn start_new_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choice) = self.selected.clone() else {
            self.picker_open = true;
            cx.notify();
            return;
        };
        let Some(root) = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).vault.read(cx).root.clone())
        else {
            self.set_error(
                self.tr(
                    cx,
                    "Open a vault before starting an agent.",
                    "Åpne et hvelv før du starter en agent.",
                )
                .to_string(),
                cx,
            );
            return;
        };
        self.confirm_launch(choice, root, window, cx);
    }

    fn add_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some(
                self.tr(cx, "Add context files", "Legg til kontekstfiler")
                    .into(),
            ),
        });
        let panel = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                let _ = window.update(|_, cx| {
                    panel.update(cx, |panel, cx| {
                        for path in paths {
                            panel.add_context(path, cx);
                        }
                    });
                });
            }
        })
        .detach();
    }

    fn render_status(&self, cx: &App) -> String {
        match self.status {
            AgentStatus::Idle => self.tr(cx, "Not connected", "Ikke tilkoblet").into(),
            AgentStatus::Connecting => self.tr(cx, "Connecting…", "Kobler til…").into(),
            AgentStatus::Ready => self.tr(cx, "Ready", "Klar").into(),
            AgentStatus::Working => self.tr(cx, "Working…", "Arbeider…").into(),
            AgentStatus::Stopping => self.tr(cx, "Stopping…", "Stopper…").into(),
            AgentStatus::Error => self.tr(cx, "Error", "Feil").into(),
        }
    }
}

impl Workspace {
    pub(crate) fn stop_agent_process(&mut self, cx: &mut Context<Self>) {
        self.agent_panel.update(cx, |panel, cx| panel.stop(cx));
    }

    fn apply_agent_write(
        &mut self,
        path: &Path,
        before: &str,
        content: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let root = self
            .vault
            .read(cx)
            .root
            .clone()
            .ok_or_else(|| "No vault is open".to_string())?;
        let guarded = agent::guard_vault_path(&root, path).map_err(|error| error.to_string())?;
        if let Some(doc) = self.docs.iter().find(|doc| {
            doc.entity
                .read(cx)
                .path
                .canonicalize()
                .unwrap_or_else(|_| doc.entity.read(cx).path.clone())
                == guarded
        }) {
            if doc.entity.read(cx).is_read_only() {
                return Err("The open file is read-only".into());
            }
            if doc.entity.read(cx).editor.read(cx).value() != before {
                return Err("The open file changed after the proposed edit".into());
            }
            doc.entity.update(cx, |doc, cx| {
                doc.restore_text(content.to_string(), window, cx);
                doc.save(cx).map_err(|error| error.to_string())
            })?;
        } else {
            match std::fs::read_to_string(&guarded) {
                Ok(current) if current != before => {
                    return Err("The file changed after the proposed edit".into())
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && before.is_empty() => {
                }
                Err(error) => return Err(error.to_string()),
            }
            std::fs::write(&guarded, content).map_err(|error| error.to_string())?;
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
        }
        Ok(())
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language(cx);
        let (choices, errors) = self.choices(cx);
        let context_root = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).vault.read(cx).root.clone());
        let has_configured_agents = choices.iter().any(|choice| choice.manifest_id.is_some());
        let current_choice = self.selected.as_ref();
        let current_name = current_choice
            .map(|choice| choice.agent.name.text(language))
            .unwrap_or(self.tr(cx, "Choose an agent", "Velg en agent"));
        let can_send = self.process.is_some()
            && self.session_id.is_some()
            && self.status != AgentStatus::Working
            && !self.stopping
            && !self.composer.read(cx).value().to_string().trim().is_empty();
        let show_picker = choices.is_empty() || self.picker_open;

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .rounded(cx.theme().radius_lg)
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context("RistaAgent")
            .capture_action::<input::Enter>(cx.listener(
                |this, enter: &input::Enter, window, cx| {
                    if !enter.shift {
                        this.send_from_composer(window, cx);
                        cx.stop_propagation();
                    }
                },
            ))
            .child(
                h_flex()
                    .h_10()
                    .px_3()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(assets::IconName::MessageSquareQuote)
                            .size_3p5()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_medium()
                            .child(self.tr(cx, "Agent", "Agent")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.render_status(cx)),
                    )
                    .child(
                        Button::new("agent-new-conversation")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Plus)
                            .tooltip(self.tr(cx, "New conversation", "Ny samtale"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_new_conversation(window, cx)
                            })),
                    )
                    .when(self.process.is_some(), |header| {
                        header.child(
                            Button::new("agent-stop")
                                .ghost()
                                .xsmall()
                                .icon(assets::IconName::Square)
                                .tooltip(self.tr(cx, "Stop agent", "Stopp agent"))
                                .disabled(self.stopping)
                                .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                        )
                    })
                    .child(
                        Button::new("agent-hide-panel")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Close)
                            .tooltip(self.tr(cx, "Hide agent panel", "Skjul agentpanel"))
                            .on_click({
                                let workspace = self.workspace.clone();
                                move |_, _, cx| {
                                    if let Some(workspace) = workspace.upgrade() {
                                        workspace.update(cx, |workspace, cx| {
                                            workspace.settings.agent_open = false;
                                            workspace.settings.save();
                                            cx.notify();
                                        });
                                    }
                                }
                            }),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .px_3()
                    .pb_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Button::new("agent-picker")
                                    .ghost()
                                    .xsmall()
                                    .icon(assets::IconName::ChevronDown)
                                    .label(current_name)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.picker_open = !this.picker_open;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("agent-add-context")
                                    .ghost()
                                    .xsmall()
                                    .icon(assets::IconName::Paperclip)
                                    .label(self.tr(cx, "Add context", "Legg til kontekst"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_files(window, cx)
                                    })),
                            ),
                    )
                    .when(show_picker, |content| {
                        let mut content = content.child(
                            v_flex()
                                .gap_1()
                                .max_h(px(150.))
                                .overflow_y_scrollbar()
                                .when(!has_configured_agents, |list| {
                                    list.child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(self.tr(
                                                cx,
                                                "Vibe is built in. Add other agents with an extension manifest.",
                                                "Vibe er innebygd. Legg til andre agenter med et utvidelsesmanifest.",
                                            )),
                                    )
                                })
                                .children(choices.iter().enumerate().map(|(index, choice)| {
                                    let source = choice
                                        .manifest_id
                                        .as_deref()
                                        .unwrap_or(self.tr(cx, "Built-in", "Innebygd"));
                                    Button::new(("agent-choice", index))
                                        .ghost()
                                        .xsmall()
                                        .w_full()
                                        .justify_start()
                                        .label(choice.agent.name.text(language).to_string())
                                        .tooltip(format!(
                                            "{source} · {}",
                                            choice.agent.id
                                        ))
                                        .on_click(cx.listener({
                                            let choice = choice.clone();
                                            move |this, _, _, cx| {
                                                this.selected = Some(choice.clone());
                                                this.picker_open = false;
                                                cx.notify();
                                            }
                                        }))
                                })),
                        );
                        for error in errors.iter().take(6) {
                            content = content.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().warning)
                                    .child(error.clone()),
                            );
                        }
                        if !has_configured_agents {
                            let extensions_dir = self
                                .workspace
                                .upgrade()
                                .and_then(|workspace| {
                                    workspace.read(cx).vault.read(cx).root.clone()
                                })
                                .map(|root| root.join(".rista/plugins"))
                                .unwrap_or_else(extensions::user_dir);
                            content = content.child(
                                Button::new("agent-open-extensions")
                                    .ghost()
                                    .xsmall()
                                    .icon(assets::IconName::FolderOpen)
                                    .label(self.tr(
                                        cx,
                                        "Open extensions folder",
                                        "Åpne utvidelsesmappen",
                                    ))
                                    .on_click(move |_, _, cx| {
                                        if std::fs::create_dir_all(&extensions_dir).is_ok() {
                                            cx.reveal_path(&extensions_dir);
                                        }
                                    }),
                            );
                        }
                        content
                    })
                    .when(!self.context.is_empty(), |content| {
                        content.child(h_flex().flex_wrap().gap_1().children(
                            self.context.iter().enumerate().map(|(index, path)| {
                                let label = path
                                    .strip_prefix(context_root.as_deref().unwrap_or(Path::new("")))
                                    .unwrap_or(path)
                                    .display()
                                    .to_string();
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .px_2()
                                    .py_1()
                                    .bg(cx.theme().group_box)
                                    .rounded(cx.theme().radius)
                                    .child(div().max_w(px(150.)).truncate().text_xs().child(label))
                                    .child(
                                        Button::new(("remove-agent-context", index))
                                            .ghost()
                                            .xsmall()
                                            .icon(assets::IconName::Close)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if index < this.context.len() {
                                                    this.remove_context(index);
                                                    cx.notify();
                                                }
                                            })),
                                    )
                            }),
                        ))
                    }),
            )
            .child(
                div()
                    .id("agent-transcript")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_3()
                    .gap_2()
                    .children(self.update.transcript.iter().map(|item| {
                        match item {
                            TranscriptItem::User(text) => div()
                                .self_flex_end()
                                .max_w_full()
                                .p_2()
                                .bg(cx.theme().group_box)
                                .rounded(cx.theme().radius)
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(self.tr(cx, "You", "Du")),
                                        )
                                        .child(div().text_sm().child(text.clone())),
                                )
                                .into_any_element(),
                            TranscriptItem::Message(text) => div()
                                .w_full()
                                .text_sm()
                                .child(text.clone())
                                .into_any_element(),
                            TranscriptItem::Thought(text) => {
                                let label = self.tr(cx, "Thinking", "Tenker");
                                div()
                                    .w_full()
                                    .child(
                                        Button::new(("agent-thought", text.len()))
                                            .ghost()
                                            .xsmall()
                                            .label(if self.thoughts_expanded {
                                                self.tr(cx, "Hide thinking", "Skjul tenking")
                                            } else {
                                                label
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.thoughts_expanded = !this.thoughts_expanded;
                                                cx.notify();
                                            })),
                                    )
                                    .when(self.thoughts_expanded, |card| {
                                        card.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(text.clone()),
                                        )
                                    })
                                    .into_any_element()
                            }
                            TranscriptItem::Tool(id) => self.render_tool(id, cx),
                            TranscriptItem::Plan => self.render_plan(cx).into_any_element(),
                        }
                    }))
                    .children(self.pending_permissions.iter().enumerate().map(
                        |(request_index, pending)| {
                            let request_id = pending.id.clone();
                            let options = pending.options.clone();
                            v_flex()
                                .w_full()
                                .gap_2()
                                .p_3()
                                .bg(cx.theme().group_box)
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(div().text_sm().font_medium().child(pending.title.clone()))
                                .child(h_flex().gap_2().children(
                                    options.into_iter().enumerate().map(
                                        |(option_index, option)| {
                                            let request_id = request_id.clone();
                                            let option_id = option.id.clone();
                                            let button_id = ((request_index as u64) << 32)
                                                | option_index as u64;
                                            let label = if option.name.is_empty() {
                                                match option.kind.as_str() {
                                                    "allow_once" => {
                                                        self.tr(cx, "Allow once", "Tillat én gang")
                                                    }
                                                    "reject_once" => self.tr(cx, "Reject", "Avvis"),
                                                    "allow_always" => {
                                                        self.tr(cx, "Always allow", "Tillat alltid")
                                                    }
                                                    "reject_always" => {
                                                        self.tr(cx, "Always reject", "Avvis alltid")
                                                    }
                                                    _ => self.tr(cx, "Choose", "Velg"),
                                                }
                                                .to_string()
                                            } else {
                                                option.name
                                            };
                                            Button::new(("agent-permission", button_id))
                                                .ghost()
                                                .xsmall()
                                                .label(label)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.answer_permission(
                                                        &request_id,
                                                        Some(&option_id),
                                                        cx,
                                                    );
                                                }))
                                        },
                                    ),
                                ))
                        },
                    ))
                    .children(
                        self.pending_writes
                            .iter()
                            .enumerate()
                            .map(|(index, pending)| {
                                let request_id = pending.id.clone();
                                let path = pending.path.clone();
                                let before = pending.before.clone();
                                let after = pending.after.clone();
                                let diff = agent::simple_diff(
                                    &path.display().to_string(),
                                    &before,
                                    &after,
                                );
                                v_flex()
                                    .w_full()
                                    .gap_2()
                                    .p_3()
                                    .bg(cx.theme().group_box)
                                    .rounded(cx.theme().radius)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .child(div().text_sm().font_medium().child(self.tr(
                                        cx,
                                        "Proposed file change",
                                        "Foreslått filendring",
                                    )))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                path.strip_prefix(
                                                    self.conversation_root
                                                        .as_deref()
                                                        .unwrap_or(Path::new("")),
                                                )
                                                .unwrap_or(&path)
                                                .display()
                                                .to_string(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .max_h(px(150.))
                                            .overflow_y_scrollbar()
                                            .child(Self::render_diff(&diff, cx)),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                Button::new(("agent-write-accept", index))
                                                    .primary()
                                                    .xsmall()
                                                    .label(self.tr(cx, "Accept", "Godta"))
                                                    .on_click(cx.listener({
                                                        let request_id = request_id.clone();
                                                        move |this, _, window, cx| {
                                                            this.answer_write(
                                                                &request_id,
                                                                true,
                                                                window,
                                                                cx,
                                                            )
                                                        }
                                                    })),
                                            )
                                            .child(
                                                Button::new(("agent-write-reject", index))
                                                    .ghost()
                                                    .xsmall()
                                                    .label(self.tr(cx, "Reject", "Avvis"))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.answer_write(
                                                                &request_id,
                                                                false,
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            ),
                                    )
                            }),
                    )
                    .when_some(self.error.clone(), |content, error| {
                        let tail = self
                            .process
                            .as_ref()
                            .map(|process| process.stderr_tail())
                            .unwrap_or_default();
                        content.child(
                            v_flex()
                                .gap_1()
                                .p_2()
                                .bg(cx.theme().group_box)
                                .rounded(cx.theme().radius)
                                .text_xs()
                                .child(error)
                                .when(!tail.is_empty(), |this| {
                                    this.child(
                                        div().text_color(cx.theme().muted_foreground).child(tail),
                                    )
                                }),
                        )
                    }),
            )
            .child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(Textarea::new(&self.composer).h(px(84.)).appearance(true))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.tr(
                                        cx,
                                        "Enter to send · Shift+Enter for a new line",
                                        "Enter for å sende · Shift+Enter for ny linje",
                                    )),
                            )
                            .child(
                                Button::new("agent-send")
                                    .primary()
                                    .xsmall()
                                    .icon(assets::IconName::ArrowUp)
                                    .label(self.tr(cx, "Send", "Send"))
                                    .disabled(!can_send)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.send_from_composer(window, cx)
                                    })),
                            ),
                    ),
            )
    }
}

impl AgentPanel {
    fn render_diff(diff: &str, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .font_family("monospace")
            .text_xs()
            .children(
                diff.lines()
                    .filter(|line| {
                        !(line.starts_with("--- ")
                            || line.starts_with("+++ ")
                            || line.starts_with("@@"))
                    })
                    .map(|line| {
                        let text = match line.split_at_checked(1) {
                            Some((sign @ ("+" | "-"), rest)) => format!("{sign} {rest}"),
                            _ => line.to_string(),
                        };
                        let row = div().w_full().px_1().rounded(cx.theme().radius).child(text);
                        if line.starts_with('+') {
                            row.text_color(cx.theme().success)
                                .bg(cx.theme().success.opacity(0.1))
                        } else if line.starts_with('-') {
                            row.text_color(cx.theme().danger)
                                .bg(cx.theme().danger.opacity(0.1))
                        } else {
                            row.text_color(cx.theme().muted_foreground)
                        }
                    }),
            )
    }

    fn render_tool(&self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some((tool_index, tool)) = self
            .update
            .tools
            .iter()
            .enumerate()
            .find(|(_, tool)| tool.id == id)
        else {
            return div().into_any_element();
        };
        let expanded = self.expanded_tools.contains(id);
        let tool = tool.clone();
        let (status_icon, status_label) = match tool.status.as_str() {
            "in_progress" => (assets::IconName::Loader, self.tr(cx, "Running", "Kjører")),
            "completed" => (assets::IconName::Check, self.tr(cx, "Done", "Ferdig")),
            "failed" | "cancelled" => (assets::IconName::X, self.tr(cx, "Failed", "Feilet")),
            _ => (assets::IconName::Circle, self.tr(cx, "Pending", "Venter")),
        };
        v_flex()
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new(("agent-tool", tool_index))
                            .ghost()
                            .xsmall()
                            .flex_1()
                            .min_w_0()
                            .justify_start()
                            .icon(if expanded {
                                assets::IconName::ChevronDown
                            } else {
                                assets::IconName::ChevronRight
                            })
                            .label(tool.title)
                            .on_click(cx.listener({
                                let id = id.to_string();
                                move |this, _, _, cx| {
                                    if !this.expanded_tools.insert(id.clone()) {
                                        this.expanded_tools.remove(&id);
                                    }
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Icon::new(status_icon)
                            .size_3p5()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(status_label),
                    ),
            )
            .when(expanded, |details| {
                details.children(tool.content.iter().map(|content| {
                    if let Some((path, old, new)) = &content.diff {
                        v_flex()
                            .gap_1()
                            .p_2()
                            .bg(cx.theme().group_box)
                            .rounded(cx.theme().radius)
                            .child(div().text_xs().child(path.clone()))
                            .child(Self::render_diff(&agent::simple_diff(path, old, new), cx))
                            .into_any_element()
                    } else {
                        div()
                            .p_2()
                            .text_xs()
                            .child(content.text.clone())
                            .into_any_element()
                    }
                }))
            })
            .into_any_element()
    }

    fn render_plan(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let plan: Vec<PlanEntry> = self.update.plan.clone();
        v_flex()
            .gap_1()
            .p_2()
            .bg(cx.theme().group_box)
            .rounded(cx.theme().radius)
            .children(plan.into_iter().map(|entry| {
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        Icon::new(match entry.status.as_str() {
                            "completed" => assets::IconName::Check,
                            "in_progress" => assets::IconName::Loader,
                            _ => assets::IconName::Circle,
                        })
                        .size_3p5()
                        .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().text_xs().child(entry.content))
            }))
    }
}

impl Focusable for AgentPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::merge_active_context;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    #[test]
    fn only_protocol_mediated_writes_are_described_as_reviewed() {
        let notice = "Approval applies only to file writes requested through ACP; the agent and its tools may change files directly.";
        assert!(notice.contains("only"));
        assert!(notice.contains("directly"));
    }

    #[test]
    fn active_context_follows_tabs_and_keeps_dismissed_notes_removed() {
        let opening = PathBuf::from("/vault/Opening.md");
        let neighbor = PathBuf::from("/vault/Neighbor.md");
        let extra = PathBuf::from("/vault/Extra.md");
        let explicit = HashSet::from([extra.clone()]);
        let dismissed = HashSet::from([opening.clone()]);
        let mut context = vec![opening.clone(), extra.clone()];

        merge_active_context(
            &mut context,
            Some(Path::new(&opening)),
            Some(Path::new(&neighbor)),
            &explicit,
            &dismissed,
        );
        assert_eq!(context, vec![neighbor.clone(), extra.clone()]);

        merge_active_context(
            &mut context,
            Some(Path::new(&neighbor)),
            Some(Path::new(&opening)),
            &explicit,
            &dismissed,
        );
        assert_eq!(context, vec![extra.clone()]);

        merge_active_context(
            &mut context,
            Some(Path::new(&opening)),
            Some(Path::new(&neighbor)),
            &explicit,
            &dismissed,
        );
        assert_eq!(context, vec![neighbor, extra]);
    }
}
