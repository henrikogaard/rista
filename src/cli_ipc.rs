use crate::cli::{CliError, CliRequest, CliResponse, MAX_FRAME_BYTES};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(test)]
const IO_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(not(test))]
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const FRAME_HEADER_BYTES: usize = 4;

#[cfg(unix)]
mod unix {
    use super::*;
    use std::fs::{self, File, OpenOptions};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::{
        DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt,
    };
    use std::os::unix::net::{UnixListener, UnixStream};

    pub(crate) struct InstanceLock(File);

    impl Drop for InstanceLock {
        fn drop(&mut self) {
            unsafe {
                libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }

    pub(crate) enum LockAttempt {
        Acquired(InstanceLock),
        Busy,
    }

    #[derive(Clone, Debug)]
    pub(crate) struct RuntimePaths {
        pub(crate) lock: PathBuf,
        pub(crate) socket: PathBuf,
    }

    pub(crate) fn runtime_paths(profile: &Path) -> Result<RuntimePaths, CliError> {
        let identity = profile_identity(profile)?;
        let uid = unsafe { libc::geteuid() };
        let directory = PathBuf::from(format!("/tmp/rista-{uid}-{identity:016x}"));
        match fs::symlink_metadata(&directory) {
            Ok(metadata) => verify_directory(&directory, &metadata)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                match builder.create(&directory) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(io_error(error)),
                }
                let metadata = fs::symlink_metadata(&directory).map_err(io_error)?;
                verify_directory(&directory, &metadata)?;
            }
            Err(error) => return Err(io_error(error)),
        }
        Ok(RuntimePaths {
            lock: directory.join("lock"),
            socket: directory.join("s"),
        })
    }

    fn profile_identity(profile: &Path) -> Result<u64, CliError> {
        let absolute = if profile.is_absolute() {
            profile.to_path_buf()
        } else {
            std::env::current_dir().map_err(io_error)?.join(profile)
        };
        let mut ancestor = absolute.as_path();
        let mut missing = Vec::new();
        let canonical_ancestor = loop {
            match ancestor.canonicalize() {
                Ok(path) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let Some(name) = ancestor.file_name() else {
                        return Err(io_error(error));
                    };
                    missing.push(name.to_os_string());
                    let Some(parent) = ancestor.parent() else {
                        return Err(io_error(error));
                    };
                    ancestor = parent;
                }
                Err(error) => return Err(io_error(error)),
            }
        };
        let identity = missing
            .into_iter()
            .rev()
            .fold(canonical_ancestor, |path, component| path.join(component));
        let mut hash = 0xcbf29ce484222325u64;
        for byte in identity.as_os_str().as_encoded_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        Ok(hash)
    }

    fn verify_directory(_path: &Path, metadata: &fs::Metadata) -> Result<(), CliError> {
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o700
        {
            return Err(CliError::new(
                "The CLI runtime directory has unsafe ownership or permissions",
                "Kjøretidsmappen for kommandolinjen har utrygt eierskap eller tillatelser",
            ));
        }
        Ok(())
    }

    pub(crate) fn try_lock(profile: &Path) -> Result<(RuntimePaths, LockAttempt), CliError> {
        let paths = runtime_paths(profile)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&paths.lock)
            .map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(CliError::new(
                "The CLI lock file has unsafe ownership or permissions",
                "Låsefilen for kommandolinjen har utrygt eierskap eller tillatelser",
            ));
        }
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result == 0 {
            return Ok((paths, LockAttempt::Acquired(InstanceLock(file))));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok((paths, LockAttempt::Busy));
        }
        Err(io_error(error))
    }

    pub(crate) fn bind_locked(
        paths: &RuntimePaths,
        _lock: &InstanceLock,
    ) -> Result<UnixListener, CliError> {
        remove_stale_socket(&paths.socket)?;
        let listener = UnixListener::bind(&paths.socket).map_err(io_error)?;
        fs::set_permissions(&paths.socket, fs::Permissions::from_mode(0o600)).map_err(io_error)?;
        let metadata = fs::symlink_metadata(&paths.socket).map_err(io_error)?;
        if !metadata.file_type().is_socket()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600
        {
            let _ = fs::remove_file(&paths.socket);
            return Err(CliError::new(
                "The CLI socket has unsafe ownership or permissions",
                "Sokkelen for kommandolinjen har utrygt eierskap eller tillatelser",
            ));
        }
        Ok(listener)
    }

    fn remove_stale_socket(socket: &Path) -> Result<(), CliError> {
        match fs::symlink_metadata(socket) {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && metadata.uid() == unsafe { libc::geteuid() }
                    && metadata.mode() & 0o777 == 0o600 =>
            {
                fs::remove_file(socket).map_err(io_error)
            }
            Ok(_) => Err(CliError::new(
                "Refusing to remove an unsafe CLI socket path",
                "Nekter å fjerne en utrygg sokkelsti for kommandolinjen",
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }

    pub(crate) fn remove_owned_socket(paths: &RuntimePaths) -> Result<(), CliError> {
        remove_stale_socket(&paths.socket)
    }

    pub(crate) fn forward(
        paths: &RuntimePaths,
        request: &CliRequest,
    ) -> Result<CliResponse, CliError> {
        validate_socket(&paths.socket)?;
        let mut stream = connect_bounded(&paths.socket)?;
        write_frame(&mut stream, request)?;
        read_frame(&mut stream)
    }

    fn validate_socket(socket: &Path) -> Result<(), CliError> {
        let metadata = fs::symlink_metadata(socket).map_err(io_error)?;
        if !metadata.file_type().is_socket()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(CliError::new(
                "The CLI socket has unsafe ownership or permissions",
                "Sokkelen for kommandolinjen har utrygt eierskap eller tillatelser",
            ));
        }
        Ok(())
    }

    fn connect_bounded(socket: &Path) -> Result<UnixStream, CliError> {
        use std::os::fd::RawFd;
        let path = socket.as_os_str().as_encoded_bytes();
        if path.len() + 1 > std::mem::size_of::<libc::sockaddr_un>().saturating_sub(2) {
            return Err(CliError::new(
                "The local CLI socket path is too long",
                "Den lokale sokkelstien for kommandolinjen er for lang",
            ));
        }
        let descriptor = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        if descriptor < 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            unsafe { libc::close(descriptor) };
            return Err(io_error(std::io::Error::last_os_error()));
        }
        let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        address.sun_family = libc::AF_UNIX as libc::sa_family_t;
        unsafe {
            std::ptr::copy_nonoverlapping(
                path.as_ptr().cast::<libc::c_char>(),
                address.sun_path.as_mut_ptr(),
                path.len(),
            );
        }
        #[cfg(target_vendor = "apple")]
        {
            address.sun_len = (std::mem::size_of::<libc::sa_family_t>() + path.len() + 1) as u8;
        }
        #[cfg(target_vendor = "apple")]
        let address_len = (2 + path.len() + 1) as libc::socklen_t;
        #[cfg(not(target_vendor = "apple"))]
        let address_len =
            (std::mem::size_of::<libc::sa_family_t>() + path.len() + 1) as libc::socklen_t;
        let result = unsafe {
            libc::connect(
                descriptor,
                (&address as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
                address_len,
            )
        };
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EINPROGRESS) {
                unsafe { libc::close(descriptor) };
                return Err(io_error(error));
            }
            let mut poll = libc::pollfd {
                fd: descriptor,
                events: libc::POLLOUT,
                revents: 0,
            };
            if unsafe { libc::poll(&mut poll, 1, IO_TIMEOUT.as_millis() as i32) } <= 0 {
                unsafe { libc::close(descriptor) };
                return Err(timeout_error());
            }
            let mut socket_error: libc::c_int = 0;
            let mut length = std::mem::size_of_val(&socket_error) as libc::socklen_t;
            if unsafe {
                libc::getsockopt(
                    descriptor,
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    (&mut socket_error as *mut libc::c_int).cast(),
                    &mut length,
                )
            } < 0
                || socket_error != 0
            {
                unsafe { libc::close(descriptor) };
                return Err(io_error(std::io::Error::from_raw_os_error(
                    if socket_error == 0 {
                        std::io::Error::last_os_error()
                            .raw_os_error()
                            .unwrap_or(libc::EIO)
                    } else {
                        socket_error
                    },
                )));
            }
        }
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags & !libc::O_NONBLOCK) } < 0
        {
            unsafe { libc::close(descriptor) };
            return Err(io_error(std::io::Error::last_os_error()));
        }
        let stream = unsafe { UnixStream::from_raw_fd(descriptor as RawFd) };
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(io_error)?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(io_error)?;
        Ok(stream)
    }
}

pub(crate) fn write_frame<T: Serialize>(
    writer: &mut impl Write,
    value: &T,
) -> Result<(), CliError> {
    let payload = serde_json::to_vec(value).map_err(|error| {
        CliError::new(
            format!("Could not encode local request: {error}"),
            format!("Kunne ikke kode lokal forespørsel: {error}"),
        )
    })?;
    if payload.len().saturating_add(FRAME_HEADER_BYTES) > MAX_FRAME_BYTES {
        return Err(frame_size_error());
    }
    let length = u32::try_from(payload.len()).map_err(|_| frame_size_error())?;
    writer.write_all(&length.to_be_bytes()).map_err(io_error)?;
    writer.write_all(&payload).map_err(io_error)
}

pub(crate) fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T, CliError> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header).map_err(io_error)?;
    let length = u32::from_be_bytes(header) as usize;
    if length.saturating_add(FRAME_HEADER_BYTES) > MAX_FRAME_BYTES {
        return Err(frame_size_error());
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).map_err(io_error)?;
    serde_json::from_slice(&payload).map_err(|error| {
        CliError::new(
            format!("Invalid local CLI request: {error}"),
            format!("Ugyldig lokal forespørsel: {error}"),
        )
    })
}

#[cfg(unix)]
pub(crate) fn coordinate_write(
    profile: &Path,
    request: &CliRequest,
    direct: impl FnOnce() -> Result<Vec<String>, CliError>,
) -> Result<Vec<String>, CliError> {
    let (paths, attempt) = unix::try_lock(profile)?;
    match attempt {
        unix::LockAttempt::Acquired(_lock) => direct(),
        unix::LockAttempt::Busy => unix::forward(&paths, request)?.result,
    }
}

#[cfg(not(unix))]
pub(crate) fn coordinate_write(
    _profile: &Path,
    _request: &CliRequest,
    _direct: impl FnOnce() -> Result<Vec<String>, CliError>,
) -> Result<Vec<String>, CliError> {
    Err(unsupported())
}

#[cfg(unix)]
pub(crate) fn serve(
    listener: std::os::unix::net::UnixListener,
    requests: smol::channel::Sender<PendingRequest>,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::Duration;

    if listener.set_nonblocking(true).is_err() {
        return;
    }
    while running.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                let request = match read_frame::<CliRequest>(&mut stream) {
                    Ok(request) => request,
                    Err(error) => {
                        let _ = write_frame(&mut stream, &CliResponse::failure(error));
                        continue;
                    }
                };
                let (reply, response) = mpsc::sync_channel(1);
                if requests.send_blocking((request, reply)).is_err() {
                    break;
                }
                let response = response
                    .recv_timeout(IO_TIMEOUT)
                    .unwrap_or_else(|_| CliResponse::failure(timeout_error()));
                let _ = write_frame(&mut stream, &response);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

#[cfg(unix)]
pub(crate) use unix::{bind_locked, forward, remove_owned_socket, try_lock, LockAttempt};

#[cfg(test)]
use unix::runtime_paths;

#[cfg(unix)]
pub(crate) type PendingRequest = (CliRequest, std::sync::mpsc::SyncSender<CliResponse>);

#[cfg(not(unix))]
pub(crate) fn unsupported() -> CliError {
    CliError::new(
        "CLI write coordination is unavailable on this platform",
        "Koordinering av kommandolinjeskriving er ikke tilgjengelig på denne plattformen",
    )
}

fn frame_size_error() -> CliError {
    CliError::new(
        "Local IPC frame exceeds the 4 MiB limit",
        "Den lokale IPC-rammen overskrider grensen på 4 MiB",
    )
}

fn timeout_error() -> CliError {
    CliError::new(
        "Timed out connecting to the running Rísta instance",
        "Tidsavbrudd ved tilkobling til den kjørende Rísta-appen",
    )
}

fn io_error(error: std::io::Error) -> CliError {
    CliError::new(
        format!("Local CLI coordination failed: {error}"),
        format!("Lokal koordinering av kommandolinjen mislyktes: {error}"),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::cli::{Command, RequestOrigin};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn profile() -> PathBuf {
        loop {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("rista-ipc-profile-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return path.join("settings.json"),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create test profile: {error}"),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn profile_identity_is_stable_before_creation_and_through_home_alias() {
        use std::os::unix::fs::symlink;

        let profile = profile();
        let parent = profile.parent().unwrap();
        let alias = parent.with_extension("alias");
        symlink(parent, &alias).unwrap();
        let unresolved = alias.join("nested").join("settings.json");
        let before = runtime_paths(&unresolved).unwrap();
        fs::create_dir_all(unresolved.parent().unwrap()).unwrap();
        fs::write(&unresolved, "{}").unwrap();
        let after = runtime_paths(&unresolved).unwrap();
        let canonical = runtime_paths(&parent.join("nested/settings.json")).unwrap();
        assert_eq!(before.socket, after.socket);
        assert_eq!(after.socket, canonical.socket);
        let _ = fs::remove_dir_all(before.socket.parent().unwrap());
        let _ = fs::remove_dir_all(parent);
        let _ = fs::remove_file(alias);
    }

    fn request() -> CliRequest {
        CliRequest {
            command: Command::Daily {
                vault: PathBuf::from("/vault"),
            },
            origin: RequestOrigin::Terminal,
        }
    }

    #[test]
    fn frames_round_trip_and_reject_oversize_lengths() {
        let request = request();
        let mut frame = Vec::new();
        write_frame(&mut frame, &request).unwrap();
        let decoded: CliRequest = read_frame(&mut frame.as_slice()).unwrap();
        assert_eq!(decoded, request);

        let oversized = (MAX_FRAME_BYTES as u32 - FRAME_HEADER_BYTES as u32 + 1)
            .to_be_bytes()
            .to_vec();
        assert!(read_frame::<CliRequest>(&mut oversized.as_slice()).is_err());
        let payload = "x".repeat(MAX_FRAME_BYTES);
        assert!(write_frame(&mut Vec::new(), &payload).is_err());
    }

    #[test]
    fn profile_lock_serializes_writers() {
        let profile = profile();
        let (paths, first) = try_lock(&profile).unwrap();
        assert!(matches!(&first, LockAttempt::Acquired(_)));
        let (_, second) = try_lock(&profile).unwrap();
        assert!(matches!(&second, LockAttempt::Busy));
        drop(first);
        let (_, third) = try_lock(&profile).unwrap();
        assert!(matches!(&third, LockAttempt::Acquired(_)));
        let _ = fs::remove_dir_all(paths.lock.parent().unwrap());
    }

    #[test]
    fn a_busy_lock_never_falls_back_to_direct_writing() {
        let profile = profile();
        let (paths, first) = try_lock(&profile).unwrap();
        let mut wrote = false;
        let response = coordinate_write(&profile, &request(), || {
            wrote = true;
            Ok(Vec::new())
        });
        assert!(response.is_err());
        assert!(!wrote);
        drop(first);
        let _ = fs::remove_dir_all(paths.lock.parent().unwrap());
    }

    #[test]
    fn unresponsive_owner_times_out_without_direct_writing() {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixListener;
        use std::thread;

        let profile = profile();
        let (paths, attempt) = try_lock(&profile).unwrap();
        let LockAttempt::Acquired(lock) = attempt else {
            panic!("expected to acquire the process lock");
        };
        let listener = UnixListener::bind(&paths.socket).unwrap();
        fs::set_permissions(&paths.socket, fs::Permissions::from_mode(0o600)).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_frame::<CliRequest>(&mut stream);
            thread::sleep(Duration::from_millis(600));
        });
        let mut wrote = false;
        let started = std::time::Instant::now();
        let response = coordinate_write(&profile, &request(), || {
            wrote = true;
            Ok(Vec::new())
        });
        assert!(response.is_err());
        assert!(!wrote);
        assert!(started.elapsed() < Duration::from_secs(2));
        server.join().unwrap();
        drop(lock);
        let _ = fs::remove_dir_all(paths.lock.parent().unwrap());
    }

    #[test]
    fn stale_socket_cleanup_refuses_non_socket_paths() {
        let profile = profile();
        let (paths, attempt) = try_lock(&profile).unwrap();
        let LockAttempt::Acquired(lock) = attempt else {
            panic!("expected to acquire the process lock");
        };
        fs::write(&paths.socket, "not a socket").unwrap();
        assert!(bind_locked(&paths, &lock).is_err());
        assert!(paths.socket.exists());
        drop(lock);
        let _ = fs::remove_dir_all(paths.lock.parent().unwrap());
    }
}
