use std::path::{Path, PathBuf};

const INSTALL_PATH: &str = "/usr/local/bin/rista";

#[derive(Debug)]
pub(crate) struct InstallError {
    english: String,
    norwegian: String,
}

impl InstallError {
    fn new(english: impl Into<String>, norwegian: impl Into<String>) -> Self {
        Self {
            english: english.into(),
            norwegian: norwegian.into(),
        }
    }

    pub(crate) fn message(&self, language: crate::settings::Language) -> &str {
        match language {
            crate::settings::Language::English => &self.english,
            crate::settings::Language::Norwegian => &self.norwegian,
        }
    }
}

pub(crate) fn install_current() -> Result<(), InstallError> {
    #[cfg(target_os = "macos")]
    {
        let executable = std::env::current_exe().map_err(io_error)?;
        let target = packaged_executable(&executable)?;
        let install_path = PathBuf::from(INSTALL_PATH);
        match install_symlink(&install_path, &target) {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
                ) =>
            {
                install_with_authorization(&install_path, &target)
            }
            Err(error) => Err(io_error(error)),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(InstallError::new(
            "The command-line tool can only be installed from the packaged macOS app",
            "Kommandolinjeverktøyet kan bare installeres fra den pakkede macOS-appen",
        ))
    }
}

fn packaged_executable(executable: &Path) -> Result<PathBuf, InstallError> {
    let executable = executable.canonicalize().map_err(io_error)?;
    let macos = executable.parent();
    let contents = macos.and_then(Path::parent);
    let bundle = contents.and_then(Path::parent);
    if executable.file_name().is_none_or(|name| name != "rista")
        || macos
            .and_then(Path::file_name)
            .is_none_or(|name| name != "MacOS")
        || contents
            .and_then(Path::file_name)
            .is_none_or(|name| name != "Contents")
        || bundle
            .and_then(Path::extension)
            .is_none_or(|extension| extension != "app")
    {
        return Err(InstallError::new(
            "Install the command from the packaged Rísta app, not a development build",
            "Installer kommandoen fra den pakkede Rísta-appen, ikke et utviklerbygg",
        ));
    }
    Ok(executable)
}

fn install_symlink(destination: &Path, target: &Path) -> std::io::Result<()> {
    validate_install_parent(destination)?;
    match std::fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let existing = std::fs::read_link(destination)?;
            let existing = if existing.is_absolute() {
                existing
            } else {
                destination
                    .parent()
                    .unwrap_or_else(|| Path::new("/"))
                    .join(existing)
            };
            if existing.canonicalize()? == target.canonicalize()? {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "A different symlink already exists",
                ))
            }
        }
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "A non-symlink command already exists",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, destination)?;
            #[cfg(not(unix))]
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Symlinks are unavailable",
            ));
            verify_installed_symlink(destination, target)
        }
        Err(error) => Err(error),
    }
}

fn validate_install_parent(destination: &Path) -> std::io::Result<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("/"));
    let mut current = PathBuf::from("/");
    for component in parent.components().skip(1) {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "The install parent is not a safe directory",
                ))
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn verify_installed_symlink(destination: &Path, target: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(destination)?;
    if !metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "The installed command is not a symlink",
        ));
    }
    let actual = std::fs::read_link(destination)?;
    let actual = if actual.is_absolute() {
        actual
    } else {
        destination
            .parent()
            .unwrap_or_else(|| Path::new("/"))
            .join(actual)
    };
    if actual.canonicalize()? == target.canonicalize()? {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "The installed symlink does not target the packaged executable",
        ))
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn apple_script_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if character.is_control() => {
                quoted.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(target_os = "macos")]
fn install_with_authorization(destination: &Path, target: &Path) -> Result<(), InstallError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("/"));
    let shell = format!(
        "if [ -L /usr/local ] || [ -e /usr/local ] && [ ! -d /usr/local ]; then exit 1; fi; \
         if [ -L {} ] || [ -e {} ] && [ ! -d {} ]; then exit 1; fi; \
         if [ ! -e {} ]; then /bin/mkdir -m 755 {}; fi; \
         if [ -L {} ] || [ -e {} ]; then exit 1; fi; \
         /bin/ln -h -s {} {} || exit 1; \
         [ \"$(/usr/bin/readlink {})\" = {} ]",
        shell_quote(&parent.to_string_lossy()),
        shell_quote(&parent.to_string_lossy()),
        shell_quote(&parent.to_string_lossy()),
        shell_quote(&parent.to_string_lossy()),
        shell_quote(&parent.to_string_lossy()),
        shell_quote(&destination.to_string_lossy()),
        shell_quote(&destination.to_string_lossy()),
        shell_quote(&target.to_string_lossy()),
        shell_quote(&destination.to_string_lossy()),
        shell_quote(&destination.to_string_lossy()),
        shell_quote(&target.to_string_lossy())
    );
    let script = format!(
        "do shell script {} with administrator privileges",
        apple_script_quote(&shell)
    );
    let status = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(io_error)?;
    if status.success() {
        verify_installed_symlink(destination, target).map_err(io_error)
    } else {
        Err(InstallError::new(
            "The command-line tool was not installed",
            "Kommandolinjeverktøyet ble ikke installert",
        ))
    }
}

fn io_error(error: std::io::Error) -> InstallError {
    InstallError::new(
        format!("Could not install the command-line tool: {error}"),
        format!("Kunne ikke installere kommandolinjeverktøyet: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        loop {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("rista-cli-install-{}-{id}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return path.canonicalize().unwrap(),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create test directory: {error}"),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn install_helper_is_idempotent_and_refuses_conflicts() {
        use std::os::unix::fs::symlink;
        let root = temp_dir();
        let target = root.join("rista");
        let other = root.join("other");
        let destination = root.join("bin/rista");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(&target, "").unwrap();
        std::fs::write(&other, "").unwrap();

        symlink(&target, &destination).unwrap();
        assert!(install_symlink(&destination, &target).is_ok());
        assert!(install_symlink(&destination, &other).is_err());

        std::fs::remove_file(&destination).unwrap();
        std::fs::write(&destination, "existing").unwrap();
        assert!(install_symlink(&destination, &target).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quoting_handles_unicode_quotes_and_shell_metacharacters() {
        assert_eq!(
            shell_quote("/Applications/Rísta's.app/rista"),
            "'/Applications/Rísta'\"'\"'s.app/rista'"
        );
        assert_eq!(
            apple_script_quote("say \"Rísta\"\nnext"),
            "\"say \\\"Rísta\\\"\\nnext\""
        );
    }

    #[test]
    fn install_refuses_a_missing_parent_without_creating_it() {
        let root = temp_dir();
        let destination = root.join("missing/bin/rista");
        let error = validate_install_parent(&destination).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(!destination.parent().unwrap().exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
