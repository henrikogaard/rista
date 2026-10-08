use anyhow::{anyhow, bail, Context, Result};
use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const MAX_ROWS: u16 = 500;
const MAX_COLS: u16 = 500;
const SCROLLBACK_ROWS: usize = 2_000;
const WRITE_QUEUE_CAPACITY: usize = 64;
const MAX_WRITE_BYTES: usize = 32 * 1024;
const MAX_CSI_BYTES: usize = 16;

struct Shared {
    parser: Mutex<vt100::Parser>,
    generation: AtomicU64,
    exited: AtomicBool,
    closing: AtomicBool,
    error: Mutex<Option<String>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            parser: Mutex::new(vt100::Parser::new(
                DEFAULT_ROWS,
                DEFAULT_COLS,
                SCROLLBACK_ROWS,
            )),
            generation: AtomicU64::new(0),
            exited: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }
}

pub struct Session {
    shared: Arc<Shared>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    child_killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    writer: SyncSender<Vec<u8>>,
}

impl Session {
    pub fn spawn(program: &str, args: &[String], cwd: &Path) -> Result<Self> {
        if program.is_empty() || program.contains('\0') {
            bail!("terminal program must be nonempty and contain no NUL bytes");
        }
        if args.iter().any(|arg| arg.contains('\0')) {
            bail!("terminal arguments must not contain NUL bytes");
        }

        let system = native_pty_system();
        let pair = system
            .openpty(pty_size(DEFAULT_ROWS, DEFAULT_COLS))
            .context("open terminal PTY")?;
        let reader = pair
            .master
            .try_clone_reader()
            .context("open terminal PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("open terminal PTY writer")?;
        let shared = Arc::new(Shared::new());
        let (writer_tx, writer_rx) = sync_channel(WRITE_QUEUE_CAPACITY);

        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        #[cfg(target_os = "macos")]
        if let Some(path) = packaged_macos_path() {
            command.env("PATH", path);
        }

        let child = pair
            .slave
            .spawn_command(command)
            .context("spawn terminal process")?;
        let child_killer = child.clone_killer();
        let child_slot = Arc::new(Mutex::new(Some(child)));

        let writer_shared = Arc::clone(&shared);
        let writer_thread = thread::Builder::new()
            .name("rista-terminal-writer".to_string())
            .spawn(move || writer_loop(writer, writer_rx, writer_shared));
        if let Err(error) = writer_thread {
            terminate_child(&child_slot);
            bail!("start terminal PTY writer thread: {error}");
        }

        let reader_shared = Arc::clone(&shared);
        let reader_writer = writer_tx.clone();
        let reader_thread = thread::Builder::new()
            .name("rista-terminal-reader".to_string())
            .spawn(move || reader_loop(reader, reader_writer, reader_shared));
        if let Err(error) = reader_thread {
            terminate_child(&child_slot);
            bail!("start terminal PTY reader thread: {error}");
        }

        let waiter_slot = Arc::clone(&child_slot);
        let waiter_shared = Arc::clone(&shared);
        let waiter = thread::Builder::new()
            .name("rista-terminal-reaper".to_string())
            .spawn(move || {
                let child = lock_recover(&waiter_slot).take();
                if let Some(mut child) = child {
                    if let Err(error) = child.wait() {
                        record_error(
                            &waiter_shared,
                            format!("terminal process wait failed: {error}"),
                        );
                    }
                }
                waiter_shared.exited.store(true, Ordering::Release);
                waiter_shared.generation.fetch_add(1, Ordering::AcqRel);
            });
        if let Err(error) = waiter {
            terminate_child(&child_slot);
            bail!("start terminal process reaper thread: {error}");
        }

        Ok(Self {
            shared,
            master: Mutex::new(Some(pair.master)),
            child_killer: Some(child_killer),
            writer: writer_tx,
        })
    }

    pub fn snapshot(&self) -> vt100::Screen {
        lock_recover(&self.shared.parser).screen().clone()
    }

    pub fn send(&self, bytes: Vec<u8>) -> Result<()> {
        if bytes.len() > MAX_WRITE_BYTES {
            bail!("terminal input exceeds the {}-byte limit", MAX_WRITE_BYTES);
        }
        if self.exited() {
            bail!("terminal process has exited");
        }
        if bytes.is_empty() {
            return Ok(());
        }

        match self.writer.try_send(bytes) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => bail!("terminal input queue is full"),
            Err(TrySendError::Disconnected(_)) => {
                if let Some(error) = self.error() {
                    bail!("terminal writer stopped: {error}");
                }
                bail!("terminal writer has stopped");
            }
        }
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let size = pty_size(rows, cols);
        {
            let master = lock_recover(&self.master);
            let master = master
                .as_ref()
                .ok_or_else(|| anyhow!("terminal PTY is closed"))?;
            master.resize(size).context("resize terminal PTY")?;
        }
        lock_recover(&self.shared.parser).set_size(size.rows, size.cols);
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn scroll(&self, delta: i32) {
        if delta == 0 {
            return;
        }
        let mut parser = lock_recover(&self.shared.parser);
        let current = parser.screen().scrollback();
        let next = if delta > 0 {
            current.saturating_add(delta as usize)
        } else {
            current.saturating_sub(delta.unsigned_abs() as usize)
        };
        if current != next {
            parser.set_scrollback(next);
            self.shared.generation.fetch_add(1, Ordering::AcqRel);
        }
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }

    pub fn exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }

    pub fn error(&self) -> Option<String> {
        lock_recover(&self.shared.error).clone()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.closing.store(true, Ordering::Release);
        if let Some(killer) = self.child_killer.as_mut() {
            let _ = killer.kill();
        }
        let master = self
            .master
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        master.take();
    }
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(1, MAX_ROWS),
        cols: cols.clamp(1, MAX_COLS),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn terminate_child(child_slot: &Arc<Mutex<Option<Box<dyn Child + Send + Sync>>>>) {
    if let Some(mut child) = lock_recover(child_slot).take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn writer_loop(
    mut writer: Box<dyn Write + Send>,
    receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    shared: Arc<Shared>,
) {
    while let Ok(bytes) = receiver.recv() {
        if let Err(error) = writer.write_all(&bytes) {
            if !shared.closing.load(Ordering::Acquire) && !is_pty_closed_error(&error) {
                record_error(&shared, format!("terminal input write failed: {error}"));
            }
            break;
        }
    }
}

fn reader_loop(mut reader: Box<dyn Read + Send>, writer: SyncSender<Vec<u8>>, shared: Arc<Shared>) {
    let mut buffer = [0_u8; 4_096];
    let mut scanner = DeviceQueryScanner::default();
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let mut parser = lock_recover(&shared.parser);
                for byte in &buffer[..count] {
                    parser.process(std::slice::from_ref(byte));
                    if let Some(query) = scanner.push(*byte) {
                        let response = match query {
                            DeviceQuery::Status => b"\x1b[0n".to_vec(),
                            DeviceQuery::CursorPosition => {
                                let (row, col) = parser.screen().cursor_position();
                                format!("\x1b[{};{}R", row + 1, col + 1).into_bytes()
                            }
                        };
                        if let Err(error) = writer.try_send(response) {
                            record_error(
                                &shared,
                                match error {
                                    TrySendError::Full(_) => {
                                        "terminal query response queue is full".to_string()
                                    }
                                    TrySendError::Disconnected(_) => {
                                        "terminal writer has stopped".to_string()
                                    }
                                },
                            );
                        }
                    }
                }
                shared.generation.fetch_add(1, Ordering::AcqRel);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if shared.closing.load(Ordering::Acquire) || is_pty_closed_error(&error) => {
                break;
            }
            Err(error) => {
                record_error(&shared, format!("terminal output read failed: {error}"));
                break;
            }
        }
    }
}

#[derive(Default)]
struct DeviceQueryScanner {
    state: ScannerState,
    csi: Vec<u8>,
}

#[derive(Default, Clone, Copy)]
enum ScannerState {
    #[default]
    Ground,
    Escape,
    Csi,
    String,
    StringEscape,
    DiscardCsi,
}

#[derive(Clone, Copy)]
enum DeviceQuery {
    Status,
    CursorPosition,
}

impl DeviceQueryScanner {
    fn push(&mut self, byte: u8) -> Option<DeviceQuery> {
        match self.state {
            ScannerState::Ground => {
                if byte == 0x1b {
                    self.state = ScannerState::Escape;
                }
            }
            ScannerState::Escape => match byte {
                b'[' => {
                    self.csi.clear();
                    self.state = ScannerState::Csi;
                }
                b']' | b'P' | b'X' | b'^' | b'_' => self.state = ScannerState::String,
                0x1b => self.state = ScannerState::Escape,
                _ => self.state = ScannerState::Ground,
            },
            ScannerState::Csi => {
                if (0x40..=0x7e).contains(&byte) {
                    let query = if self.csi.as_slice() == b"5" && byte == b'n' {
                        Some(DeviceQuery::Status)
                    } else if self.csi.as_slice() == b"6" && byte == b'n' {
                        Some(DeviceQuery::CursorPosition)
                    } else {
                        None
                    };
                    self.state = ScannerState::Ground;
                    return query;
                }
                if byte == 0x1b {
                    self.state = ScannerState::Escape;
                } else if self.csi.len() < MAX_CSI_BYTES {
                    self.csi.push(byte);
                } else {
                    self.state = ScannerState::DiscardCsi;
                }
            }
            ScannerState::String => match byte {
                0x07 => self.state = ScannerState::Ground,
                0x1b => self.state = ScannerState::StringEscape,
                _ => {}
            },
            ScannerState::StringEscape => match byte {
                b'\\' | 0x07 => self.state = ScannerState::Ground,
                0x1b => self.state = ScannerState::StringEscape,
                _ => self.state = ScannerState::String,
            },
            ScannerState::DiscardCsi => {
                if (0x40..=0x7e).contains(&byte) {
                    self.state = ScannerState::Ground;
                }
            }
        }
        None
    }
}

fn record_error(shared: &Shared, message: String) {
    let mut error = lock_recover(&shared.error);
    if error.is_none() {
        *error = Some(message);
        shared.generation.fetch_add(1, Ordering::AcqRel);
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn is_pty_closed_error(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::UnexpectedEof
    ) {
        return true;
    }
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(5)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
fn packaged_macos_path() -> Option<std::ffi::OsString> {
    let executable = std::env::current_exe().ok()?;
    if !executable
        .to_string_lossy()
        .contains(".app/Contents/MacOS/")
    {
        return None;
    }

    let mut paths =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    let mut candidates = vec![
        Path::new("/opt/homebrew/bin").to_path_buf(),
        Path::new("/opt/homebrew/sbin").to_path_buf(),
        Path::new("/usr/local/bin").to_path_buf(),
        Path::new("/usr/local/sbin").to_path_buf(),
    ];
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        candidates.push(home.join(".local/bin"));
        candidates.push(home.join("bin"));
    }
    for candidate in candidates {
        if candidate.is_dir() && !paths.iter().any(|path| path == &candidate) {
            paths.push(candidate);
        }
    }
    std::env::join_paths(paths).ok()
}

#[cfg(test)]
mod tests {
    use super::{DeviceQuery, DeviceQueryScanner};
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn parser_handles_ansi_split_across_reads_and_tracks_modes() {
        let mut parser = vt100::Parser::new(8, 24, 16);
        parser.process(b"hello \x1b[");
        parser.process(b"31mred");
        parser.process(b"\x1b[?2004h");

        assert!(parser.screen().contents().contains("hello red"));
        assert!(parser.screen().bracketed_paste());
    }

    #[test]
    fn parser_tracks_cursor_and_alternate_screen() {
        let mut parser = vt100::Parser::new(8, 24, 16);
        parser.process(b"\x1b[3;5H");
        assert_eq!(parser.screen().cursor_position(), (2, 4));

        parser.process(b"\x1b[?1049hALT");
        assert!(parser.screen().alternate_screen());
        assert!(parser.screen().contents().contains("ALT"));

        parser.process(b"\x1b[?1049l");
        assert!(!parser.screen().alternate_screen());
    }

    #[test]
    fn parser_resizes_without_exceeding_requested_dimensions() {
        let mut parser = vt100::Parser::new(8, 24, 16);
        parser.set_size(40, 100);

        assert_eq!(parser.screen().size(), (40, 100));
    }

    #[test]
    fn device_queries_are_recognized_when_split_across_reads() {
        let mut scanner = DeviceQueryScanner::default();
        assert!(scanner.push(0x1b).is_none());
        assert!(scanner.push(b'[').is_none());
        assert!(scanner.push(b'5').is_none());
        assert!(matches!(scanner.push(b'n'), Some(DeviceQuery::Status)));

        assert!(scanner.push(0x1b).is_none());
        assert!(scanner.push(b'[').is_none());
        assert!(scanner.push(b'6').is_none());
        assert!(matches!(
            scanner.push(b'n'),
            Some(DeviceQuery::CursorPosition)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn pty_roundtrip_writes_and_reads_child_output_and_reaps_exit() {
        let session = super::Session::spawn("/usr/bin/tee", &[], Path::new("/"))
            .expect("spawn tee through a PTY");
        session.resize(40, 100).expect("resize PTY");
        assert_eq!(session.snapshot().size(), (40, 100));
        session
            .send(b"PTY_ROUNDTRIP\n\x04".to_vec())
            .expect("write to PTY child");
        let deadline = Instant::now() + Duration::from_secs(3);

        while Instant::now() < deadline {
            if session.snapshot().contents().contains("PTY_ROUNDTRIP") && session.exited() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(session.snapshot().contents().contains("PTY_ROUNDTRIP"));
        assert!(session.exited());
        assert_eq!(session.error(), None);
    }
}
