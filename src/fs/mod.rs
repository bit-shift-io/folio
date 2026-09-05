//! Filesystem core for the browser: listing, search, preview, and safe path
//! joining. Every function takes the server's root directory plus a
//! caller-supplied relative path and never follows a path that could escape
//! the root (`safe_join` rejects absolute paths and `..` components).

use std::io;
use std::path::{Path, PathBuf};

/// One node in the file browser tree. The tree is returned flat with `depth`
/// so the client can indent without building a nested structure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileTreeEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub depth: usize,
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

/// Directories skipped when walking the tree and when searching. Kept in one
/// place so `list_dir` and `search_files` always agree on what to hide.
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

/// Joins a caller-supplied relative path onto `root`, refusing absolute paths
/// and `..` traversal components. Returns None when unsafe so handlers can
/// reject the request before touching the filesystem.
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
    {
        return None;
    }
    Some(root.join(rel_path))
}

/// Errors from mutating operations, split so handlers can distinguish a
/// rejected path (bad request) from an actual filesystem failure.
#[derive(Debug)]
pub enum FsError {
    EscapedRoot,
    Io(io::Error),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::EscapedRoot => write!(f, "path escapes the root directory"),
            FsError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FsError {}

/// Lists the immediate children of a directory inside the root. Empty `dir`
/// means the root itself. `SKIP_DIRS` are hidden so navigation stays fast and
/// focused. Directories are returned first, then files, each alphabetically.
pub fn list_dir(root: &Path, dir: &str) -> Vec<FileTreeEntry> {
    let Some(base) = safe_join(root, dir) else {
        return Vec::new();
    };

    let Ok(read) = std::fs::read_dir(&base) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let rel = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        match item.file_type() {
            Ok(ft) if ft.is_dir() => entries.push(FileTreeEntry {
                name,
                path: rel,
                is_dir: true,
                depth: 0,
            }),
            Ok(ft) if ft.is_file() => entries.push(FileTreeEntry {
                name,
                path: rel,
                is_dir: false,
                depth: 0,
            }),
            _ => {}
        }
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

/// Recursively searches the root for files whose path contains `query`
/// (case-insensitive substring match). Returns at most `limit` results,
/// skipping the same directories as `list_dir`.
pub fn search_files(root: &Path, query: &str, limit: usize) -> Vec<FileTreeEntry> {
    let q = query.to_lowercase();
    let mut results = Vec::new();
    let mut stack = vec![String::new()];
    while let Some(dir) = stack.pop() {
        let base = if dir.is_empty() {
            root.to_path_buf()
        } else {
            root.join(&dir)
        };
        let Ok(read) = std::fs::read_dir(&base) else {
            continue;
        };
        let mut children = Vec::new();
        for item in read.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            let rel = if dir.is_empty() {
                name.clone()
            } else {
                format!("{dir}/{name}")
            };
            match item.file_type() {
                Ok(ft) if ft.is_dir() => children.push((name, rel, true)),
                Ok(ft) if ft.is_file() => children.push((name, rel, false)),
                _ => {}
            }
        }
        children.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        for (name, rel, is_dir) in children {
            if is_dir {
                stack.push(rel);
            } else if rel.to_lowercase().contains(&q) {
                results.push(FileTreeEntry {
                    name,
                    path: rel,
                    is_dir: false,
                    depth: 0,
                });
                if results.len() >= limit {
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
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        if TEXT_EXTS.contains(&ext.as_str()) {
            return "text/plain";
        }
        return "application/octet-stream";
    }
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
        _ => "application/octet-stream",
    }
}

/// Image extensions render inline in the preview pane. Driven by the shared
/// `IMAGE_EXTS` list so preview behavior stays aligned with `mime_for_path`.
pub fn is_image_path(path: &str) -> bool {
    mime_for_path(path).starts_with("image/")
}

/// Reads a file for the preview pane, detecting binary contents and image
/// files. Errors are folded into the payload's `error` field so the handler
/// can return a 200 with a graceful client-side message.
pub fn get_file_content(root: &Path, path: &str) -> FileContent {
    let Some(full) = safe_join(root, path) else {
        return FileContent {
            path: path.to_string(),
            size: 0,
            is_binary: false,
            is_image: false,
            content: String::new(),
            error: "path escapes the root directory".to_string(),
        };
    };
    let meta = match std::fs::metadata(&full) {
        Ok(m) => m,
        Err(e) => {
            return FileContent {
                path: path.to_string(),
                size: 0,
                is_binary: false,
                is_image: false,
                content: String::new(),
                error: format!("failed to read {path}: {e}"),
            }
        }
    };
    let bytes = match std::fs::read(&full) {
        Ok(b) => b,
        Err(e) => {
            return FileContent {
                path: path.to_string(),
                size: 0,
                is_binary: false,
                is_image: false,
                content: String::new(),
                error: format!("failed to read {path}: {e}"),
            }
        }
    };
    let is_image = is_image_path(path);
    let is_binary = !is_image && bytes.contains(&0u8);
    let content = if is_binary || is_image {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    FileContent {
        path: path.to_string(),
        size: meta.len(),
        is_binary,
        is_image,
        content,
        error: String::new(),
    }
}

fn io_error(msg: String) -> FsError {
    FsError::Io(io::Error::new(io::ErrorKind::InvalidInput, msg))
}

/// Deletes a file or directory (recursively) inside the root. Symlinks are
/// removed as links, never followed.
pub fn delete_path(root: &Path, rel: &str) -> Result<(), FsError> {
    if rel.is_empty() {
        return Err(io_error("cannot delete the root directory".into()));
    }
    let full = safe_join(root, rel).ok_or(FsError::EscapedRoot)?;
    let meta = std::fs::symlink_metadata(&full).map_err(FsError::Io)?;
    if meta.file_type().is_symlink() {
        std::fs::remove_file(&full).map_err(FsError::Io)
    } else if meta.is_dir() {
        std::fs::remove_dir_all(&full).map_err(FsError::Io)
    } else {
        std::fs::remove_file(&full).map_err(FsError::Io)
    }
}

/// Renames (or moves) a file or directory from one relative path to another,
/// both inside the root.
pub fn rename_path(root: &Path, from: &str, to: &str) -> Result<(), FsError> {
    if from.is_empty() {
        return Err(io_error("cannot rename the root directory".into()));
    }
    if to.is_empty() {
        return Err(io_error("rename target may not be empty".into()));
    }
    let src = safe_join(root, from).ok_or(FsError::EscapedRoot)?;
    let dst = safe_join(root, to).ok_or(FsError::EscapedRoot)?;
    std::fs::rename(&src, &dst).map_err(FsError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_traversal_and_absolute_paths() {
        let root = Path::new("/srv");
        assert_eq!(
            safe_join(root, "a/b.txt"),
            Some(PathBuf::from("/srv/a/b.txt"))
        );
        assert_eq!(safe_join(root, ""), Some(PathBuf::from("/srv")));
        assert!(safe_join(root, "../etc/passwd").is_none());
        assert!(safe_join(root, "a/../../etc/passwd").is_none());
        assert!(safe_join(root, "/etc/passwd").is_none());
    }

    #[test]
    fn list_dir_returns_dirs_first_then_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), "x").unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        std::fs::create_dir(dir.path().join("zdir")).unwrap();
        std::fs::create_dir(dir.path().join("adir")).unwrap();

        let entries = list_dir(dir.path(), "");
        assert!(
            entries[0].is_dir && entries[1].is_dir && !entries[2].is_dir,
            "dirs must sort before files: {entries:?}"
        );
        assert_eq!(entries[0].name, "adir");
        assert_eq!(entries[1].name, "zdir");
        assert_eq!(entries[2].name, "a.txt");
        assert_eq!(entries[3].name, "b.txt");
    }

    #[test]
    fn list_dir_hides_skip_dirs_and_filters_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "pub fn f() {}\n").unwrap();

        let root = list_dir(dir.path(), "");
        let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"src"));
        assert!(!names.contains(&"node_modules"), "got: {names:?}");

        let sub = list_dir(dir.path(), "src");
        let paths: Vec<&str> = sub.iter().map(|e| e.path.as_str()).collect();
        assert!(
            paths.contains(&"src/main.rs") && paths.contains(&"src/lib.rs"),
            "got: {paths:?}"
        );
    }

    #[test]
    fn list_dir_traversal_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_dir(dir.path(), "../etc").is_empty());
        assert!(list_dir(dir.path(), "/etc").is_empty());
    }

    #[test]
    fn search_finds_files_case_insensitively_across_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/sub")).unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(dir.path().join("README.MD"), "# Hello\n").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("src/sub/lib.rs"), "pub fn f() {}\n").unwrap();

        let results = search_files(dir.path(), "READme", 200);
        let paths: Vec<&str> = results.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"README.MD"), "got: {paths:?}");

        let rs = search_files(dir.path(), ".rs", 200);
        let rs_paths: Vec<&str> = rs.iter().map(|e| e.path.as_str()).collect();
        assert!(
            rs_paths.contains(&"src/main.rs") && rs_paths.contains(&"src/sub/lib.rs"),
            "got: {rs_paths:?}"
        );
        assert!(!rs_paths.contains(&"Cargo.toml"));
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

        let text = get_file_content(dir.path(), "a.txt");
        assert_eq!(text.content, "hello world\n");
        assert!(!text.is_binary && !text.is_image);
        assert_eq!(text.size, 12);

        let bin = get_file_content(dir.path(), "b.bin");
        assert!(bin.is_binary);
        assert_eq!(bin.content, "");
    }

    #[test]
    fn get_file_content_reports_errors_inline() {
        let dir = tempfile::tempdir().unwrap();
        let missing = get_file_content(dir.path(), "nope.txt");
        assert!(!missing.error.is_empty());

        let traversal = get_file_content(dir.path(), "../secret.txt");
        assert_eq!(traversal.error, "path escapes the root directory");
    }

    #[test]
    fn delete_removes_files_and_directories_recursively() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join("d/sub")).unwrap();
        std::fs::write(dir.path().join("d/sub/f.txt"), "x").unwrap();

        delete_path(dir.path(), "a.txt").unwrap();
        assert!(!dir.path().join("a.txt").exists());

        delete_path(dir.path(), "d").unwrap();
        assert!(!dir.path().join("d").exists());
    }

    #[test]
    fn delete_rejects_root_and_traversal() {
        let dir = tempfile::tempdir().unwrap();
        assert!(delete_path(dir.path(), "").is_err());
        assert!(matches!(delete_path(dir.path(), "../x"), Err(FsError::EscapedRoot)));
    }

    #[test]
    fn rename_moves_and_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();

        rename_path(dir.path(), "a.txt", "sub/b.txt").unwrap();
        assert!(dir.path().join("sub/b.txt").exists());
        assert!(!dir.path().join("a.txt").exists());

        assert!(matches!(
            rename_path(dir.path(), "sub/b.txt", "../escape.txt"),
            Err(FsError::EscapedRoot)
        ));
        assert!(matches!(
            rename_path(dir.path(), "../out", "x.txt"),
            Err(FsError::EscapedRoot)
        ));
    }
}