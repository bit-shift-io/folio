//! Filesystem core for the browser: listing, search, preview, and mutations.
//! All functions operate on absolute paths; `--root` on the CLI only sets the
//! initial directory the browser opens on, it is not a confinement boundary.

use std::io;
use std::path::Path;

/// One node in the file browser. `path` is an absolute path; `depth` is always
/// `0` from the server (the client layouts rows itself).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileTreeEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub depth: usize,
    /// MIME type for icon selection: extension tables, or a magic-byte sniff
    /// for extensionless files. Directories report `inode/directory`.
    pub mime: String,
}

/// Payload for a single file's preview contents.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileContent {
    pub path: String,
    /// Raw byte count of the file (used for binary sizing).
    pub size: u64,
    pub is_binary: bool,
    pub is_image: bool,
    /// UTF-8 text content; empty for binary/image files.
    pub content: String,
    /// Non-empty when the read fails so the client can surface it inline.
    pub error: String,
}

/// Directory names skipped by *search* (a performance guard for the recursive
/// walk). Directory listing shows everything — this is a full file browser.
pub const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    ".idea",
    ".vscode",
    ".DS_Store",
];

/// Pseudo-filesystem roots that recursive search never enters. Only applied
/// when the search base is `/` itself, so normal trees are never affected.
pub const PSEUDO_ROOTS: &[&str] = &["proc", "sys", "dev", "run"];

/// Extensions treated as renderable text by the preview pane.
pub const TEXT_EXTS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "c", "cpp", "h", "hpp", "go", "java",
    "rb", "php", "sh", "bash", "zsh", "fish", "vim", "lua", "r", "swift", "kt",
    "cs", "fs", "hs", "ex", "exs", "erl", "clj", "lisp", "el", "jl",
    "toml", "yaml", "yml", "json", "jsonc", "json5", "xml", "html", "htm",
    "css", "scss", "less", "sql", "graphql", "proto", "md", "txt", "csv",
    "ini", "cfg", "conf", "env", "gitignore", "gitattributes", "dockerignore",
    "dockerfile", "makefile", "cmake", "nix", "zig",
];

/// Extensions rendered inline as images by the preview pane.
pub const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico", "avif", "tiff",
    "tif", "psd", "ai", "eps",
];

/// Errors while listing a directory, split so handlers can map them to status
/// codes (404 for missing paths, 400 for non-directories).
#[derive(Debug)]
pub enum ListError {
    NotFound,
    NotADirectory,
    Io(io::Error),
}

impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListError::NotFound => write!(f, "no such directory"),
            ListError::NotADirectory => write!(f, "not a directory"),
            ListError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ListError {}

/// Errors from mutating operations.
#[derive(Debug)]
pub enum FsError {
    InvalidInput(String),
    Io(io::Error),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::InvalidInput(m) => write!(f, "{m}"),
            FsError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FsError {}

impl From<io::Error> for FsError {
    fn from(e: io::Error) -> Self {
        FsError::Io(e)
    }
}

fn invalid(msg: &str) -> FsError {
    FsError::InvalidInput(msg.to_string())
}

/// Lists the immediate children of an absolute directory. Directories come
/// first, then files, each alphabetically. Symlinks are followed for
/// classification so a link to a folder behaves like a folder and a link to a
/// file like a file; permission failures yield `Io`.
pub fn list_dir(dir: &Path) -> Result<Vec<FileTreeEntry>, ListError> {
    let meta = std::fs::symlink_metadata(dir).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => ListError::NotFound,
        _ => ListError::Io(e),
    })?;
    if !meta.is_dir() {
        return Err(ListError::NotADirectory);
    }

    let read = std::fs::read_dir(dir).map_err(ListError::Io)?;
    let mut entries = Vec::new();
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let ft = match item.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let is_dir = if ft.is_symlink() {
            std::fs::metadata(item.path()).map(|m| m.is_dir()).unwrap_or(false)
        } else {
            ft.is_dir()
        };
        if is_dir || ft.is_file() || ft.is_symlink() {
            let mime = if is_dir {
                "inode/directory".to_string()
            } else {
                mime_for_entry(&dir.join(&name), &name)
            };
            entries.push(FileTreeEntry {
                path: dir.join(&name).display().to_string(),
                name,
                is_dir,
                depth: 0,
                mime,
            });
        }
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

/// Recursively searches `base` for files whose path contains `query`
/// (case-insensitive substring match). Returns at most `limit` results with
/// absolute paths, skipping `SKIP_DIRS` and, when the base is `/`, the
/// pseudo-filesystem roots.
pub fn search_files(base: &Path, query: &str, limit: usize) -> Vec<FileTreeEntry> {
    let q = query.to_lowercase();
    let skip_pseudofs = base == Path::new("/");
    let mut results = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children = Vec::new();
        for item in read.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if skip_pseudofs && dir == *base && PSEUDO_ROOTS.contains(&name.as_str()) {
                continue;
            }
            match item.file_type() {
                Ok(ft) if ft.is_dir() => children.push((name, true)),
                Ok(ft) if ft.is_file() => children.push((name, false)),
                _ => {}
            }
        }
        children.sort_by(|a, b| {
            b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });
        for (name, is_dir) in children {
            let full = dir.join(&name);
            if is_dir {
                stack.push(full);
            } else if name.to_lowercase().contains(&q) {
                let mime = mime_for_entry(&full, &name);
                results.push(FileTreeEntry {
                    name,
                    path: full.display().to_string(),
                    is_dir: false,
                    depth: 0,
                    mime,
                });
                if results.len() >= limit {
                    results.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
                    return results;
                }
            }
        }
    }
    results.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    results
}

/// Returns the MIME type for a file path based on its extension, reusing the
/// shared extension tables so `is_image_path`, preview, and raw serving always
/// agree.
pub fn mime_for_path(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "tiff" | "tif" => "image/tiff",
        "psd" => "image/vnd.adobe.photoshop",

        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" | "tgz" => "application/gzip",
        "bz2" => "application/x-bzip2",
        "xz" => "application/x-xz",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "deb" => "application/x-deb",
        "rpm" => "application/x-rpm",
        "iso" => "application/x-iso9660-image",
        "jar" => "application/java-archive",

        "mp3" => "audio/mpeg",
        "ogg" | "oga" | "opus" => "application/ogg",
        "flac" => "audio/flac",
        "wav" => "audio/x-wav",
        "aac" => "audio/aac",
        "m4a" => "audio/mp4",
        "mid" | "midi" => "audio/midi",
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mpg" | "mpeg" => "video/mpeg",

        "exe" => "application/x-executable",
        "so" | "o" => "application/x-sharedlib",

        e if TEXT_EXTS.contains(&e) => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Image extensions render inline in the preview pane.
pub fn is_image_path(path: &str) -> bool {
    mime_for_path(path).starts_with("image/")
}

/// MIME type used for icon selection in listings. Extension-backed types reuse
/// `mime_for_path`; extensionless files fall back to a magic-byte sniff so a
/// bare `configure`, `run`, or `app` still gets a meaningful icon.
pub fn mime_for_entry(path: &Path, name: &str) -> String {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !ext.is_empty() {
        return mime_for_path(name).to_string();
    }
    sniff_mime(path)
}

fn sniff_mime(path: &Path) -> String {
    use std::io::Read;
    let mut buf = [0u8; 512];
    let n = match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => n,
        Err(_) => return "text/plain".to_string(),
    };
    let head = &buf[..n];
    let (_, rest) = head.split_first().unwrap();

    // Textual formats: skip a UTF-8 BOM and leading whitespace before looking
    // at the first meaningful byte (`{`/`[` = JSON, `<` = markup).
    let mut i = if head.starts_with(&[0xef, 0xbb, 0xbf]) { 3 } else { 0 };
    while i < head.len() && matches!(head[i], b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c) {
        i += 1;
    }
    let body = &head[i..];
    let body_low = body.to_ascii_lowercase();

    if body.starts_with(b"{") || body.starts_with(b"[") {
        "application/json"
    } else if body_low.starts_with(b"<html") || body_low.starts_with(b"<!doctype html") {
        "text/html"
    } else if body.starts_with(b"<?xml") || body.starts_with(b"<") {
        "application/xml"
    } else if head.starts_with(&[0x7f, b'E', b'L', b'F']) {
        "application/x-executable"
    } else if head.starts_with(b"MZ") {
        "application/x-executable"
    } else if head.starts_with(b"#!") {
        "text/x-script"
    } else if head.starts_with(&[0x50, 0x4b, 0x03, 0x04]) {
        "application/zip"
    } else if head.starts_with(&[0x1f, 0x8b]) {
        "application/gzip"
    } else if head.starts_with(&[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c]) {
        "application/x-7z-compressed"
    } else if head.starts_with(&[0x42, 0x5a, 0x68]) {
        "application/x-bzip2"
    } else if head.len() >= 262 && &head[257..262] == b"ustar" {
        "application/x-tar"
    } else if head.starts_with(b"%PDF") {
        "application/pdf"
    } else if head.starts_with(&[0x89]) && rest.starts_with(b"PNG") {
        "image/png"
    } else if head.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if head.starts_with(b"GIF8") {
        "image/gif"
    } else if head.starts_with(b"RIFF") && head.len() >= 12 && &head[8..12] == b"WEBP" {
        "image/webp"
    } else if head.starts_with(b"OggS") {
        "application/ogg"
    } else if head.starts_with(b"fLaC") {
        "audio/flac"
    } else if head.starts_with(b"RIFF") && head.len() >= 12 && &head[8..12] == b"WAVE" {
        "audio/x-wav"
    } else if head.starts_with(b"RIFF") && head.len() >= 12 && &head[8..12] == b"AVI " {
        "video/x-msvideo"
    } else if head.len() >= 8 && &head[4..8] == b"ftyp" {
        "video/mp4"
    } else if head.starts_with(b"ID3") {
        "audio/mpeg"
    } else if head.len() >= 2 && head[0] == 0xff && (head[1] & 0xe0) == 0xe0 {
        "audio/mpeg"
    } else {
        "text/plain"
    }
    .to_string()
}

/// Reads a file for the preview pane, detecting binary contents and image
/// files. Errors are folded into the payload's `error` field so the handler
/// can return a 200 with a graceful client-side message.
pub fn get_file_content(path: &Path) -> FileContent {
    let path_str = path.display().to_string();
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return FileContent {
                path: path_str.clone(),
                size: 0,
                is_binary: false,
                is_image: false,
                content: String::new(),
                error: format!("failed to read {path_str}: {e}"),
            }
        }
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return FileContent {
                path: path_str.clone(),
                size: 0,
                is_binary: false,
                is_image: false,
                content: String::new(),
                error: format!("failed to read {path_str}: {e}"),
            }
        }
    };
    let is_image = is_image_path(&path_str);
    let is_binary = !is_image && bytes.contains(&0u8);
    let content = if is_binary || is_image {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    FileContent {
        path: path_str,
        size: meta.len(),
        is_binary,
        is_image,
        content,
        error: String::new(),
    }
}

/// Deletes a file or directory (recursively) at an absolute path. Symlinks are
/// removed as links, never followed. Refuses to delete the filesystem root.
pub fn delete_path(path: &Path) -> Result<(), FsError> {
    if path == Path::new("/") {
        return Err(invalid("cannot delete the root directory"));
    }
    let meta = std::fs::symlink_metadata(path).map_err(FsError::Io)?;
    if meta.file_type().is_symlink() {
        std::fs::remove_file(path).map_err(FsError::Io)
    } else if meta.is_dir() {
        std::fs::remove_dir_all(path).map_err(FsError::Io)
    } else {
        std::fs::remove_file(path).map_err(FsError::Io)
    }
}

/// Renames (or moves) a file or directory between two absolute paths. Refuses
/// to rename the filesystem root.
pub fn rename_path(from: &Path, to: &Path) -> Result<(), FsError> {
    if from == Path::new("/") {
        return Err(invalid("cannot rename the root directory"));
    }
    if to.as_os_str().is_empty() {
        return Err(invalid("rename target may not be empty"));
    }
    std::fs::rename(from, to).map_err(FsError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_fields_classify_extensionless_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("script"), "#!/bin/sh\necho hi\n").unwrap();
        std::fs::write(dir.path().join("blob"), [0x7f, b'E', b'L', b'F', 2, 1, 1, 0]).unwrap();
        std::fs::write(dir.path().join("plain"), "just text\n").unwrap();
        std::fs::write(dir.path().join("notes.md"), "# hi\n").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();

        let entries = list_dir(dir.path()).unwrap();
        let by_name = |n: &str| entries.iter().find(|e| e.name == n).unwrap();
        assert_eq!(by_name("script").mime, "text/x-script");
        assert_eq!(by_name("blob").mime, "application/x-executable");
        assert_eq!(by_name("plain").mime, "text/plain");
        assert_eq!(by_name("notes.md").mime, "text/plain");
        assert_eq!(by_name("subdir").mime, "inode/directory");
    }

    #[test]
    fn mime_fields_sniff_json_xml_and_html() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("project"), "{\n  \"type\": \"project\"\n}\n").unwrap();
        std::fs::write(dir.path().join("session"), "[ \"a\", \"b\" ]\n").unwrap();
        std::fs::write(dir.path().join("feed"), "<?xml version=\"1.0\"?>\n<root/>\n").unwrap();
        std::fs::write(dir.path().join("page"), "<!DOCTYPE html>\n<html></html>\n").unwrap();

        let entries = list_dir(dir.path()).unwrap();
        let by_name = |n: &str| entries.iter().find(|e| e.name == n).unwrap();
        assert_eq!(by_name("project").mime, "application/json");
        assert_eq!(by_name("session").mime, "application/json");
        assert_eq!(by_name("feed").mime, "application/xml");
        assert_eq!(by_name("page").mime, "text/html");
    }

    #[test]
    fn list_dir_returns_dirs_first_then_files_absolute() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), "x").unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        std::fs::create_dir(dir.path().join("zdir")).unwrap();
        std::fs::create_dir(dir.path().join("adir")).unwrap();

        let entries = list_dir(dir.path()).unwrap();
        assert!(
            entries[0].is_dir && entries[1].is_dir && !entries[2].is_dir,
            "dirs must sort before files: {entries:?}"
        );
        assert_eq!(entries[0].name, "adir");
        assert_eq!(entries[1].name, "zdir");
        assert_eq!(entries[2].name, "a.txt");
        assert_eq!(entries[3].name, "b.txt");

        let root_s = dir.path().display().to_string();
        assert_eq!(entries[0].path, format!("{root_s}/adir"));
        assert_eq!(entries[2].path, format!("{root_s}/a.txt"));
    }

    #[test]
    fn list_dir_shows_everything_including_junk_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();

        let names: Vec<String> = list_dir(dir.path())
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(names.contains(&"src".to_string()));
        assert!(
            names.contains(&"node_modules".to_string()),
            "a full browser must not hide dirs: {names:?}"
        );
    }

    #[test]
    fn list_dir_errors_for_missing_and_non_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "x").unwrap();

        assert!(matches!(
            list_dir(&dir.path().join("nope")),
            Err(ListError::NotFound)
        ));
        assert!(matches!(
            list_dir(&dir.path().join("f.txt")),
            Err(ListError::NotADirectory)
        ));
    }

    #[test]
    fn list_dir_follows_dir_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let entries = list_dir(dir.path()).unwrap();
        let link_entry = entries.iter().find(|e| e.name == "link").unwrap();
        assert!(link_entry.is_dir, "symlink to a dir must show as a dir");
    }

    #[test]
    fn search_finds_files_case_insensitively_across_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/sub")).unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(dir.path().join("README.MD"), "# Hello\n").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("src/sub/lib.rs"), "pub fn f() {}\n").unwrap();
        let root_s = dir.path().display().to_string();

        let results = search_files(dir.path(), "READme", 200);
        let paths: Vec<&str> = results.iter().map(|e| e.path.as_str()).collect();
        assert!(
            paths.contains(&format!("{root_s}/README.MD").as_str()),
            "got: {paths:?}"
        );

        let rs = search_files(dir.path(), ".rs", 200);
        let rs_paths: Vec<&str> = rs.iter().map(|e| e.path.as_str()).collect();
        assert!(
            rs_paths.contains(&format!("{root_s}/src/main.rs").as_str())
                && rs_paths.contains(&format!("{root_s}/src/sub/lib.rs").as_str()),
            "got: {rs_paths:?}"
        );
        assert!(!rs_paths.contains(&format!("{root_s}/Cargo.toml").as_str()));
    }

    #[test]
    fn search_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("file{i}.rs")), "x").unwrap();
        }
        let results = search_files(dir.path(), ".rs", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn search_skips_skip_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/secret.rs"), "x").unwrap();
        let results = search_files(dir.path(), "secret", 200);
        assert!(results.is_empty());
    }

    #[test]
    fn get_file_content_detects_text_binary_and_image() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
        std::fs::write(dir.path().join("b.bin"), [0u8, 1, 2, 0, 4]).unwrap();

        let text = get_file_content(&dir.path().join("a.txt"));
        assert_eq!(text.content, "hello world\n");
        assert!(!text.is_binary && !text.is_image);
        assert_eq!(text.size, 12);

        let bin = get_file_content(&dir.path().join("b.bin"));
        assert!(bin.is_binary);
        assert_eq!(bin.content, "");
    }

    #[test]
    fn get_file_content_reports_errors_inline() {
        let dir = tempfile::tempdir().unwrap();
        let missing = get_file_content(&dir.path().join("nope.txt"));
        assert!(!missing.error.is_empty());
    }

    #[test]
    fn delete_removes_files_and_directories_recursively() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join("d/sub")).unwrap();
        std::fs::write(dir.path().join("d/sub/f.txt"), "x").unwrap();

        delete_path(&dir.path().join("a.txt")).unwrap();
        assert!(!dir.path().join("a.txt").exists());

        delete_path(&dir.path().join("d")).unwrap();
        assert!(!dir.path().join("d").exists());
    }

    #[test]
    fn delete_rejects_fs_root() {
        assert!(delete_path(Path::new("/")).is_err());
    }

    #[test]
    fn rename_moves_and_rejects() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();

        rename_path(&dir.path().join("a.txt"), &dir.path().join("sub/b.txt")).unwrap();
        assert!(dir.path().join("sub/b.txt").exists());
        assert!(!dir.path().join("a.txt").exists());

        assert!(matches!(
            rename_path(&dir.path().join("sub/b.txt"), &dir.path().join("nope/x.txt")),
            Err(FsError::Io(_))
        ));
        assert!(matches!(rename_path(Path::new("/"), Path::new("/x")), Err(FsError::InvalidInput(_))));
    }
}