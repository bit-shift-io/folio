//! Installed-application discovery and "open with" launching.
//!
//! Reads freedesktop `.desktop` entries from the standard application
//! directories. `/open` may only launch an app the server itself enumerated
//! via [`list_apps`]; the target is passed as a single argv element of a
//! detached process spawned directly — never through a shell. The Exec line
//! comes from a `.desktop` file on disk and is parsed per the freedesktop
//! spec (field codes substituted, quoting respected).

use std::collections::HashMap;
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

/// The fields of a `[Desktop Entry]` group that folio actually reads. Parsed
/// by hand from the flat INI-ish format; only the main group is considered and
/// localized `Key[lang]=` variants are ignored.
#[derive(Default, Debug, PartialEq, Eq)]
struct Fields {
    /// Desktop-entry id: the file name without its `.desktop` suffix.
    appid: String,
    type_: String,
    name: String,
    exec: String,
    try_exec: Option<String>,
    icon: String,
    hidden: bool,
    no_display: bool,
    terminal: bool,
    mime_types: Vec<String>,
    /// KDE's `X-KDE-AliasFor`: the appid of the entry this one aliases.
    alias_for: Option<String>,
}

/// Reads the `[Desktop Entry]` group of a `.desktop` file. Comments, blank
/// lines and every other group (`[Desktop Action …]`) are skipped, and only
/// unlocalized keys are considered. Returns `None` when the file cannot be
/// read or has no `[Desktop Entry]` group at all.
fn read_fields(path: &Path) -> Option<Fields> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut f = Fields {
        appid: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string(),
        ..Fields::default()
    };
    let mut in_main = false;
    let mut seen_group = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            seen_group |= in_main;
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
            "Name" => f.name = value.to_string(),
            "Exec" => f.exec = value.to_string(),
            "TryExec" => f.try_exec = Some(value.to_string()),
            "Icon" => f.icon = value.to_string(),
            "Hidden" => f.hidden = value == "true",
            "NoDisplay" => f.no_display = value == "true",
            "Terminal" => f.terminal = value == "true",
            "MimeType" => f.mime_types.extend(
                value
                    .split(';')
                    .map(str::trim)
                    .filter(|m| !m.is_empty())
                    .map(str::to_string),
            ),
            "X-KDE-AliasFor" => f.alias_for = Some(value.to_string()),
            _ => {}
        }
    }
    seen_group.then_some(f)
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

/// What one `.desktop` file turned out to be.
enum Scanned {
    /// A launchable, visible app, plus its appid for alias lookups.
    Keep(AppEntry, String),
    /// A hidden KDE alias helper to fold into another entry later.
    Helper(PathBuf),
    /// Not an app folio will offer: wrong type, hidden, terminal, unlaunchable.
    Skip,
}

/// Vets a single `.desktop` file and builds the dropdown entry for it, if any.
fn scan_entry(path: PathBuf) -> Scanned {
    if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
        return Scanned::Skip;
    }
    let Some(f) = read_fields(&path) else {
        return Scanned::Skip;
    };
    if f.type_ != "Application" || f.hidden || f.terminal || f.exec.trim().is_empty() {
        return Scanned::Skip;
    }
    // flatpak apps would have to be launched through a sandbox shim; skip them
    // rather than spawn something that outlives the request.
    if f.exec.starts_with("flatpak run") {
        return Scanned::Skip;
    }
    if f.no_display {
        return match f.alias_for {
            Some(_) => Scanned::Helper(path),
            None => Scanned::Skip,
        };
    }

    let Some(first) = split_exec(&f.exec).into_iter().next() else {
        return Scanned::Skip;
    };
    let available = match f.try_exec.as_deref() {
        Some(t) => resolvable(t),
        None => resolvable(&first),
    };
    if !available {
        return Scanned::Skip;
    }

    let name = if f.name.is_empty() {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    } else {
        f.name.clone()
    };
    let mut mime_types = f.mime_types.clone();
    if !mime_types.contains(&"inode/directory".to_string()) && accepts_files_or_urls(&f.exec) {
        mime_types.push("inode/directory".to_string());
    }
    let appid = f.appid;
    Scanned::Keep(
        AppEntry {
            id: path.display().to_string(),
            name,
            mime_types,
        },
        appid,
    )
}

/// Enumerates installed applications: `Type=Application`, visible, non-
/// terminal, with a launchable command. Sorted by name for a stable dropdown.
pub fn list_apps() -> Vec<AppEntry> {
    let mut apps: Vec<AppEntry> = Vec::new();
    let mut by_appid: HashMap<String, usize> = HashMap::new();
    let mut helpers: Vec<PathBuf> = Vec::new();

    for dir in app_dirs() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            match scan_entry(entry.path()) {
                Scanned::Keep(app, appid) => {
                    by_appid.insert(appid, apps.len());
                    apps.push(app);
                }
                Scanned::Helper(path) => helpers.push(path),
                Scanned::Skip => {}
            }
        }
    }

    merge_alias_helpers(&mut apps, &helpers, &by_appid);
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

/// Folds the MIME types of KDE alias helpers into the entries they alias. A
/// helper is a `NoDisplay` entry whose `X-KDE-AliasFor` names another appid;
/// its types are extra file associations for that app, not a separate entry.
fn merge_alias_helpers(
    apps: &mut [AppEntry],
    helpers: &[PathBuf],
    by_appid: &HashMap<String, usize>,
) {
    for path in helpers {
        let Some(f) = read_fields(path) else {
            continue;
        };
        let Some(alias) = f.alias_for.as_deref() else {
            continue;
        };
        let alias_appid = alias
            .trim()
            .strip_suffix(".desktop")
            .unwrap_or(alias.trim());
        if alias_appid.is_empty() {
            continue;
        }
        let Some(&idx) = by_appid.get(alias_appid) else {
            continue;
        };
        for m in &f.mime_types {
            if !m.is_empty() && !apps[idx].mime_types.contains(m) {
                apps[idx].mime_types.push(m.clone());
            }
        }
    }
}

/// True when the Exec line carries `%F`/`%U` (or their lowercase forms), the
/// freedesktop field codes that mean "substitute a file/URL argument here".
/// Apps that take such arguments can be handed any file URI, including
/// `file://` URIs for directories, so they are folder-openable even when the
/// desktop entry's `MimeType` field doesn't list `inode/directory`.
fn accepts_files_or_urls(exec: &str) -> bool {
    let bytes = exec.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        if i + 1 >= bytes.len() {
            break;
        }
        if bytes[i + 1] == b'%' {
            i += 2;
            continue;
        }
        match bytes[i + 1] {
            b'F' | b'U' | b'f' | b'u' => return true,
            _ => {}
        }
        i += 1;
    }
    false
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
    let Some(entry) = read_fields(Path::new(id)) else {
        return Err(LaunchError::NoExec(id.to_string()));
    };
    if entry.exec.trim().is_empty() {
        return Err(LaunchError::NoExec(id.to_string()));
    }
    let (mut argv, used_file) = expand_exec(
        split_exec(&entry.exec),
        &entry.name,
        &entry.icon,
        id,
        target,
    );
    if argv.is_empty() {
        return Err(LaunchError::NoExec(id.to_string()));
    }
    // Append target path if no file code was found in Exec line
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

    /// Writes a `.desktop` file into a fresh temp applications dir and parses
    /// it with `read_fields`.
    fn parse_entry(name: &str, body: &str) -> Fields {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, body).unwrap();
        read_fields(&path).expect("entry must parse")
    }

    #[test]
    fn read_fields_reads_the_desktop_entry_group_only() {
        let f = parse_entry(
            "app.desktop",
            "# a leading comment\n\
             [Desktop Entry]\n\
             Type=Application\n\
             Name=App\n\
             \n\
             Exec=/bin/true\n\
             TryExec=/bin/true\n\
             Icon=my-icon\n\
             \n\
             [Desktop Action new-window]\n\
             Name=New Window\n\
             Exec=/bin/true --new\n\
             MimeType=image/png;\n\
             \n\
             [Desktop Entry Extra]\n\
             Name=Not Read\n",
        );
        assert_eq!(f.appid, "app");
        assert_eq!(f.type_, "Application");
        assert_eq!(f.name, "App");
        assert_eq!(f.exec, "/bin/true");
        assert_eq!(f.try_exec.as_deref(), Some("/bin/true"));
        assert_eq!(f.icon, "my-icon");
        assert!(f.mime_types.is_empty(), "action groups are ignored");
    }

    #[test]
    fn read_fields_prefers_the_unlocalized_name() {
        let f = parse_entry(
            "app.desktop",
            "[Desktop Entry]\n\
             Type=Application\n\
             Name[de]=Anwendung\n\
             Name=Application\n\
             Name[fr]=Application\n\
             Exec=/bin/true\n",
        );
        assert_eq!(f.name, "Application");
    }

    #[test]
    fn read_fields_parses_booleans_literally() {
        let f = parse_entry(
            "app.desktop",
            "[Desktop Entry]\n\
             Type=Application\n\
             Exec=/bin/true\n\
             Hidden=true\n\
             NoDisplay=TRUE\n\
             Terminal=True\n",
        );
        assert!(f.hidden, "only the literal `true` is true");
        assert!(!f.no_display, "`TRUE` is not `true`");
        assert!(!f.terminal, "`True` is not `true`");

        let f = parse_entry(
            "app.desktop",
            "[Desktop Entry]\nType=Application\nExec=/bin/true\nHidden=false\n",
        );
        assert!(!f.hidden);
    }

    #[test]
    fn read_fields_splits_mime_type_on_semicolons() {
        let f = parse_entry(
            "app.desktop",
            "[Desktop Entry]\n\
             Type=Application\n\
             Exec=/bin/true\n\
             MimeType=text/plain; ;image/png;;text/markdown;\n",
        );
        assert_eq!(
            f.mime_types,
            vec![
                "text/plain".to_string(),
                "image/png".to_string(),
                "text/markdown".to_string()
            ],
            "empties are dropped and entries are trimmed"
        );
    }

    #[test]
    fn read_fields_captures_the_kde_alias_field() {
        let f = parse_entry(
            "org.example.app_png.desktop",
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=App\n\
             Exec=/bin/true %F\n\
             NoDisplay=true\n\
             X-KDE-AliasFor=org.example.app.desktop\n\
             MimeType=image/png;\n",
        );
        assert_eq!(f.appid, "org.example.app_png");
        assert_eq!(f.alias_for.as_deref(), Some("org.example.app.desktop"));
        assert!(f.no_display);
    }

    #[test]
    fn read_fields_returns_none_for_unreadable_or_typeless_entries() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_fields(&dir.path().join("missing.desktop")).is_none());

        // A file with no `[Desktop Entry]` group at all has nothing to read.
        let stray = dir.path().join("stray.desktop");
        std::fs::write(&stray, "just some text\n").unwrap();
        assert!(read_fields(&stray).is_none());
    }

    #[test]
    fn list_apps_reads_xdg_data_home_and_filters_hidden() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        write(
            "noexec.desktop",
            "[Desktop Entry]\nType=Application\nName=No Exec\n",
        );
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
            editor.mime_types.iter().any(|m| m == "inode/directory"),
            "Exec has %F so inode/directory should be injected; got: {:?}",
            editor.mime_types
        );
        assert!(
            apps.iter().all(|a| a.name != "Hidden App"
                && a.name != "No Display"
                && a.name != "Term App"
                && a.name != "Not An App"
                && a.name != "No Exec"),
            "filtered entries leaked: {apps:?}"
        );
    }

    #[test]
    fn list_apps_merges_kde_alias_helpers_into_main_entry() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let apps_dir = dir.path().join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();
        std::fs::write(
            apps_dir.join("org.example.myapp.desktop"),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=My App\n\
             Exec=/bin/true\n\
             TryExec=/bin/true\n\
             MimeType=application/x-myapp;\n",
        )
        .unwrap();
        std::fs::write(
            apps_dir.join("myapp_png.desktop"),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=My App\n\
             Exec=/bin/true %F\n\
             TryExec=/bin/true\n\
             NoDisplay=true\n\
             X-KDE-AliasFor=org.example.myapp.desktop\n\
             MimeType=image/png;\n",
        )
        .unwrap();
        std::fs::write(
            apps_dir.join("myapp_jpeg.desktop"),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=My App\n\
             Exec=/bin/true %F\n\
             NoDisplay=true\n\
             X-KDE-AliasFor=org.example.myapp.desktop\n\
             MimeType=image/jpeg;\n",
        )
        .unwrap();

        let old = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", dir.path());
        let apps = list_apps();
        match old {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }

        let myapp = apps
            .iter()
            .find(|a| a.name == "My App")
            .expect("my app present");
        assert!(myapp
            .mime_types
            .contains(&"application/x-myapp".to_string()));
        assert!(myapp.mime_types.contains(&"image/png".to_string()));
        assert!(myapp.mime_types.contains(&"image/jpeg".to_string()));
        assert_eq!(
            apps.iter().filter(|a| a.name == "My App").count(),
            1,
            "alias helpers must not appear as separate entries"
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

        let (argv, used) = expand_exec(
            split_exec("icon-app %i %c %k"),
            "MyApp",
            "my-icon",
            "/x/y.desktop",
            file,
        );
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
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

    #[test]
    fn accepts_files_or_urls_detects_field_codes() {
        assert!(accepts_files_or_urls("zed %U"));
        assert!(accepts_files_or_urls("code --folder-uri %U"));
        assert!(accepts_files_or_urls("editor %F"));
        assert!(accepts_files_or_urls("editor %f %f"));
        assert!(accepts_files_or_urls("editor %u"));
        assert!(!accepts_files_or_urls("ls %m"));
        assert!(!accepts_files_or_urls("echo hello"));
        assert!(!accepts_files_or_urls("%%U"));
        assert!(!accepts_files_or_urls("100%% done"));
    }

    #[test]
    fn list_apps_injects_inode_directory_for_exec_with_file_or_url_codes() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let apps_dir = dir.path().join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();

        std::fs::write(
            apps_dir.join("url_editor.desktop"),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=URL Editor\n\
             Exec=/bin/true %U\n\
             MimeType=text/plain;\n",
        )
        .unwrap();

        std::fs::write(
            apps_dir.join("file_editor.desktop"),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=File Editor\n\
             Exec=/bin/true %F\n\
             MimeType=text/plain;\n",
        )
        .unwrap();

        std::fs::write(
            apps_dir.join("plain.desktop"),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Plain App\n\
             Exec=/bin/true\n\
             MimeType=text/plain;\n",
        )
        .unwrap();

        std::fs::write(
            apps_dir.join("explicit_dir.desktop"),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Dir App\n\
             Exec=/bin/true %U\n\
             MimeType=inode/directory;text/plain;\n",
        )
        .unwrap();

        let old = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", dir.path());
        let apps = list_apps();
        match old {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }

        let url_editor = apps.iter().find(|a| a.name == "URL Editor").unwrap();
        assert!(
            url_editor
                .mime_types
                .contains(&"inode/directory".to_string()),
            "url_editor: {:?}",
            url_editor.mime_types
        );

        let file_editor = apps.iter().find(|a| a.name == "File Editor").unwrap();
        assert!(
            file_editor
                .mime_types
                .contains(&"inode/directory".to_string()),
            "file_editor: {:?}",
            file_editor.mime_types
        );

        let plain = apps.iter().find(|a| a.name == "Plain App").unwrap();
        assert!(
            !plain.mime_types.contains(&"inode/directory".to_string()),
            "plain should not get inode/directory: {:?}",
            plain.mime_types
        );

        let explicit = apps.iter().find(|a| a.name == "Dir App").unwrap();
        assert_eq!(
            explicit
                .mime_types
                .iter()
                .filter(|m| *m == "inode/directory")
                .count(),
            1,
            "no duplicate inode/directory: {:?}",
            explicit.mime_types
        );
    }
}
