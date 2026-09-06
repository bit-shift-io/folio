//! Installed-application discovery and "open with" launching.
//!
//! Reads freedesktop `.desktop` entries from the standard application
//! directories. `/open` may only launch an app the server itself enumerated
//! via [`list_apps`]; the target is passed as a single argv element of a
//! detached process spawned directly — never through a shell. The Exec line
//! comes from a `.desktop` file on disk and is parsed per the freedesktop
//! spec (field codes substituted, quoting respected).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// One installed application as surfaced to the dropdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppEntry {
    /// Absolute path of the `.desktop` file; also the id used by `/open`.
    pub id: String,
    /// Display name from the desktop entry.
    pub name: String,
    /// MIME types the app advertises, used to surface suggestions first.
    #[serde(default)]
    pub mime_types: Vec<String>,
}

/// Failure modes for [`open_with`].
#[derive(Debug)]
pub enum LaunchError {
    /// The id is not a currently enumerated application.
    NotListed(String),
    /// The target path does not exist.
    MissingTarget(String),
    /// The app's launch command is empty or unusable.
    NoExec(String),
    /// The process could not be spawned.
    Spawn(String),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::NotListed(id) => write!(f, "no such application: {id}"),
            LaunchError::MissingTarget(p) => write!(f, "no such file: {p}"),
            LaunchError::NoExec(id) => write!(f, "application has no usable command: {id}"),
            LaunchError::Spawn(e) => write!(f, "failed to launch: {e}"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Standard directories holding `.desktop` files (user first, then system).
fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(d).join("applications"));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share/applications"));
    }
    match std::env::var("XDG_DATA_DIRS") {
        Ok(v) => {
            for p in v.split(':').filter(|p| !p.is_empty()) {
                dirs.push(PathBuf::from(p).join("applications"));
            }
        }
        Err(_) => {
            dirs.push(PathBuf::from("/usr/local/share/applications"));
            dirs.push(PathBuf::from("/usr/share/applications"));
        }
    }
    dirs
}

/// Raw fields of one `.desktop` file, before usability filtering.
#[derive(Default)]
struct Fields {
    id: String,
    type_: String,
    name: String,
    exec: String,
    try_exec: Option<String>,
    icon: String,
    hidden: bool,
    no_display: bool,
    terminal: bool,
    mime_types: Vec<String>,
}

fn read_fields(path: &Path) -> Option<Fields> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut f = Fields {
        id: path.display().to_string(),
        ..Fields::default()
    };
    let mut in_main = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Type" => f.type_ = value.to_string(),
            "Name" => {
                if f.name.is_empty() {
                    f.name = value.to_string();
                }
            }
            "Exec" => f.exec = value.to_string(),
            "TryExec" => f.try_exec = Some(value.to_string()),
            "Icon" => f.icon = value.to_string(),
            "Hidden" => f.hidden = value == "true",
            "NoDisplay" => f.no_display = value == "true",
            "Terminal" => f.terminal = value == "true",
            "MimeType" => {
                f.mime_types.extend(
                    value
                        .split(';')
                        .map(str::trim)
                        .filter(|m| !m.is_empty())
                        .map(str::to_string),
                );
            }
            _ => {}
        }
    }
    Some(f)
}

fn is_flatpak(exec: &str) -> bool {
    exec.starts_with("flatpak run")
}

fn resolvable(token: &str) -> bool {
    if token.contains('/') {
        return Path::new(token).is_file();
    }
    find_in_path(token).is_some()
}

fn find_in_path(program: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Enumerates installed applications: `Type=Application`, visible, non-
/// terminal, with a launchable command. Sorted by name for a stable dropdown.
pub fn list_apps() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    for dir in app_dirs() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(fields) = read_fields(&path) else {
                continue;
            };
            if fields.type_ != "Application"
                || fields.hidden
                || fields.no_display
                || fields.terminal
                || fields.exec.trim().is_empty()
                || is_flatpak(&fields.exec)
            {
                continue;
            }
            let Some(first) = split_exec(&fields.exec).into_iter().next() else {
                continue;
            };
            let available = match &fields.try_exec {
                Some(t) => resolvable(t),
                None => resolvable(&first),
            };
            if !available {
                continue;
            }
            let name = if fields.name.is_empty() {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            } else {
                fields.name
            };
            apps.push(AppEntry {
                id: fields.id,
                name,
                mime_types: fields.mime_types,
            });
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

/// Tokenizes an Exec line per the freedesktop spec: whitespace-separated,
/// double quotes group tokens, and a backslash escapes the next character.
fn split_exec(exec: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_token = true;
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    in_token = true;
                }
            }
            c if c.is_whitespace() && !quoted => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            other => {
                current.push(other);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

/// Replaces any field codes inside a single token (mixed-token edge case);
/// exact-code tokens are handled by [`expand_exec`] instead.
fn substitute_codes(tok: &str, name: &str, id: &str, file: &str) -> (String, bool) {
    let mut out = String::with_capacity(tok.len());
    let mut used_file = false;
    let mut chars = tok.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push('%');
            break;
        };
        match next {
            '%' => out.push('%'),
            'f' | 'u' | 'F' | 'U' => {
                out.push_str(file);
                used_file = true;
            }
            'c' => out.push_str(name),
            'k' => out.push_str(id),
            'i' | 'd' | 'D' | 'n' | 'N' | 'v' => {}
            other => {
                out.push('%');
                out.push(other);
            }
        }
    }
    (out, used_file)
}

/// Builds the launch argv for an Exec line. Returns `(argv, used_file_code)`
/// so the caller knows whether the target path still needs appending.
fn expand_exec(
    tokens: Vec<String>,
    name: &str,
    icon: &str,
    id: &str,
    file: &Path,
) -> (Vec<String>, bool) {
    let file_s = file.display().to_string();
    let mut argv = Vec::new();
    let mut used_file = false;
    for token in tokens {
        match token.as_str() {
            "%f" | "%u" | "%F" | "%U" => {
                argv.push(file_s.clone());
                used_file = true;
            }
            "%i" => {
                if !icon.is_empty() {
                    argv.push("--icon".to_string());
                    argv.push(icon.to_string());
                }
            }
            "%c" => argv.push(name.to_string()),
            "%k" => argv.push(id.to_string()),
            "%d" | "%D" | "%n" | "%N" | "%v" => {}
            "%%" => argv.push("%".to_string()),
            _ => {
                let (sub, used) = substitute_codes(&token, name, id, &file_s);
                used_file |= used;
                if !sub.is_empty() {
                    argv.push(sub);
                }
            }
        }
    }
    (argv, used_file)
}

/// Launches the enumerated app `id` with `target` as the opened file, unless
/// the Exec line already consumed a `%f`/`%u`/`%F`/`%U` code. The process is
/// detached and reaped on a background thread; nothing is waited on or
/// captured.
pub fn open_with(id: &str, target: &Path) -> Result<(), LaunchError> {
    if !list_apps().iter().any(|a| a.id == id) {
        return Err(LaunchError::NotListed(id.to_string()));
    }
    if std::fs::symlink_metadata(target).is_err() {
        return Err(LaunchError::MissingTarget(target.display().to_string()));
    }
    let fields = read_fields(Path::new(id)).ok_or_else(|| LaunchError::NoExec(id.to_string()))?;
    if fields.exec.trim().is_empty() {
        return Err(LaunchError::NoExec(id.to_string()));
    }
    let (mut argv, used_file) = expand_exec(
        split_exec(&fields.exec),
        &fields.name,
        &fields.icon,
        &fields.id,
        target,
    );
    if argv.is_empty() {
        return Err(LaunchError::NoExec(id.to_string()));
    }
    if !used_file {
        argv.push(target.display().to_string());
    }
    let program = argv.remove(0);
    let mut cmd = Command::new(&program);
    cmd.args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| LaunchError::Spawn(e.to_string()))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that read or write `XDG_DATA_HOME` must serialize: the variable
    /// is process-global and would race across parallel test threads.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn list_apps_reads_xdg_data_home_and_filters_hidden() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let apps_dir = dir.path().join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();
        let write = |name: &str, body: &str| std::fs::write(apps_dir.join(name), body).unwrap();
        write(
            "editor.desktop",
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=My Editor\n\
             Exec=/bin/sh %F\n\
             TryExec=/bin/sh\n\
             MimeType=text/plain;text/markdown;\n",
        );
        write(
            "hidden.desktop",
            "[Desktop Entry]\nType=Application\nName=Hidden App\nExec=/bin/false\nHidden=true\n",
        );
        write(
            "nondisplay.desktop",
            "[Desktop Entry]\nType=Application\nName=No Display\nExec=/bin/false\nNoDisplay=true\n",
        );
        write(
            "terminal.desktop",
            "[Desktop Entry]\nType=Application\nName=Term App\nExec=/bin/false\nTerminal=true\n",
        );
        write(
            "notapp.desktop",
            "[Desktop Entry]\nType=Link\nName=Not An App\nExec=/bin/false\n",
        );
        write("noexec.desktop", "[Desktop Entry]\nType=Application\nName=No Exec\n");
        write("stuff.txt", "ignored");

        let old = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", dir.path());
        let apps = list_apps();
        match old {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }

        let editor = apps.iter().find(|a| a.name == "My Editor");
        assert!(editor.is_some(), "expected editor among {apps:?}");
        let editor = editor.unwrap();
        assert_eq!(
            editor.id,
            apps_dir.join("editor.desktop").display().to_string()
        );
        assert!(
            editor.mime_types.iter().any(|m| m == "text/plain"),
            "got: {:?}",
            editor.mime_types
        );
        assert!(
            apps
                .iter()
                .all(|a| a.name != "Hidden App"
                    && a.name != "No Display"
                    && a.name != "Term App"
                    && a.name != "Not An App"
                    && a.name != "No Exec"),
            "filtered entries leaked: {apps:?}"
        );
    }

    #[test]
    fn split_exec_handles_quotes_and_escapes() {
        assert_eq!(
            split_exec("gnome-text-editor %U"),
            vec!["gnome-text-editor", "%U"]
        );
        assert_eq!(
            split_exec(r#"env B=1 foo "a b" c"#),
            vec!["env", "B=1", "foo", "a b", "c"]
        );
        assert_eq!(
            split_exec(r#"app --opt="x y" tail"#),
            vec!["app", "--opt=x y", "tail"]
        );
        assert_eq!(split_exec(r"app esc\ space"), vec!["app", "esc space"]);
    }

    #[test]
    fn expand_exec_substitutes_codes_and_appends_path() {
        let file = Path::new("/data/readme.md");

        let (argv, used) = expand_exec(split_exec("editor %f"), "Editor", "", "", file);
        assert!(used);
        assert_eq!(argv, vec!["editor", "/data/readme.md"]);

        let (argv, used) =
            expand_exec(split_exec("icon-app %i %c %k"), "MyApp", "my-icon", "/x/y.desktop", file);
        assert!(!used);
        assert_eq!(
            argv,
            vec!["icon-app", "--icon", "my-icon", "MyApp", "/x/y.desktop"]
        );

        let (argv, used) = expand_exec(split_exec("plain-app"), "P", "", "", file);
        assert!(!used);
        assert_eq!(argv, vec!["plain-app"]);

        let (argv, used) = expand_exec(split_exec(r#"list %F "--" tail"#), "P", "", "", file);
        assert!(used);
        assert_eq!(argv, vec!["list", "/data/readme.md", "--", "tail"]);
    }

    #[cfg(unix)]
    #[test]
    fn open_with_runs_an_enumerated_app_detached() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let apps_dir = dir.path().join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();
        let script = dir.path().join("opener.sh");
        let marker = dir.path().join("marker");
        std::fs::write(
            &script,
            format!("#!/bin/sh\ncp \"$1\" '{}'\n", marker.display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            apps_dir.join("opener.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=Opener\nExec={} %F\nTryExec={}\n",
                script.display(),
                script.display()
            ),
        )
        .unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, "hello").unwrap();

        let old = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", dir.path());
        let result = open_with(
            &apps_dir.join("opener.desktop").display().to_string(),
            &target,
        );
        match old {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        result.expect("launch must succeed");

        for _ in 0..150 {
            if marker.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "hello");
    }

    #[test]
    fn open_with_rejects_unknown_apps_and_missing_targets() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let apps_dir = dir.path().join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();
        std::fs::write(
            apps_dir.join("app.desktop"),
            "[Desktop Entry]\nType=Application\nName=A\nExec=/bin/true\nTryExec=/bin/true\n",
        )
        .unwrap();
        let id = apps_dir.join("app.desktop").display().to_string();

        let old = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", dir.path());

        assert!(matches!(
            open_with("/none/such.desktop", &dir.path().join("f.txt")),
            Err(LaunchError::NotListed(_))
        ));
        assert!(matches!(
            open_with(&id, &dir.path().join("missing.txt")),
            Err(LaunchError::MissingTarget(_))
        ));

        std::fs::write(dir.path().join("f.txt"), "x").unwrap();
        assert!(open_with(&id, &dir.path().join("f.txt")).is_ok());

        match old {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
    }
}