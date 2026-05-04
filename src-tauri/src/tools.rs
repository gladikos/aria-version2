use std::path::{Path, PathBuf};
use serde::Serialize;

// ─── Path safety ──────────────────────────────────────────────────────────────

pub fn check_path_safety(path: &str) -> Result<(), String> {
    if path.starts_with("\\\\") {
        return Err("UNC network paths are not supported.".into());
    }
    let norm = path.to_lowercase().replace('/', "\\");
    let blocked = [
        "\\windows\\system32\\config",
        "\\windows\\system32\\sam",
        "\\windows\\ntds",
    ];
    if blocked.iter().any(|b| norm.contains(b)) {
        return Err("Access to this system path is restricted.".into());
    }
    Ok(())
}

// ─── Timestamp helper ─────────────────────────────────────────────────────────

fn age_string(meta: &std::fs::Metadata) -> String {
    let Ok(modified) = meta.modified() else { return "unknown".into() };
    let Ok(then) = modified.duration_since(std::time::UNIX_EPOCH) else { return "unknown".into() };
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else { return "unknown".into() };
    let days = now.as_secs().saturating_sub(then.as_secs()) / 86400;
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..=30 => format!("{days} days ago"),
        31..=364 => format!("{} months ago", days / 30),
        _ => format!("{} years ago", days / 365),
    }
}

// ─── list_directory ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_directory: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    pub modified_at: String,
}

#[derive(Debug, Serialize)]
pub struct ListDirectoryResult {
    pub path: String,
    pub entries: Vec<DirEntry>,
    pub truncated: bool,
}

const LIST_LIMIT: usize = 200;

pub fn list_directory(path: &str) -> Result<ListDirectoryResult, String> {
    check_path_safety(path)?;
    let p = Path::new(path);
    if !p.exists() { return Err(format!("Path does not exist: {path}")); }
    if !p.is_dir() { return Err(format!("Not a directory: {path}")); }

    let mut dirs: Vec<DirEntry> = Vec::new();
    let mut files: Vec<DirEntry> = Vec::new();
    let mut over_limit = false;

    for entry in std::fs::read_dir(p).map_err(|e| format!("Cannot read directory: {e}"))? {
        let Ok(entry) = entry else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        let e = DirEntry {
            is_directory: meta.is_dir(),
            size_bytes: meta.is_file().then(|| meta.len()),
            modified_at: age_string(&meta),
            name,
        };
        if meta.is_dir() { dirs.push(e) } else { files.push(e) }
        if dirs.len() + files.len() > LIST_LIMIT {
            over_limit = true;
            break;
        }
    }

    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let mut entries: Vec<DirEntry> = dirs;
    entries.extend(files);
    entries.truncate(LIST_LIMIT);

    Ok(ListDirectoryResult { path: path.into(), entries, truncated: over_limit })
}

// ─── search_filesystem ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SearchMatch {
    pub path: String,
    pub is_directory: bool,
    pub modified_at: String,
    #[serde(skip)]
    pub depth: usize,
}

pub const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", ".git", "appdata",
    "$recycle.bin", "windows", "system volume information",
    "programdata", "program files", "program files (x86)",
];

fn should_skip(name: &str) -> bool {
    let low = name.to_lowercase();
    low.starts_with('.') || SKIP_DIRS.contains(&low.as_str())
}

fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(p) = dirs::desktop_dir()  { roots.push(p); }
    if let Some(p) = dirs::document_dir() { roots.push(p); }
    if let Some(p) = dirs::download_dir() { roots.push(p); }
    if let Some(p) = dirs::home_dir()     { roots.push(p); }
    #[cfg(target_os = "windows")]
    for c in b'C'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", c as char));
        if drive.exists() { roots.push(drive); }
    }
    roots
}

pub fn search_filesystem(
    query: &str,
    root: Option<&str>,
    max_results: u32,
) -> Result<Vec<SearchMatch>, String> {
    if query.is_empty() { return Err("Query cannot be empty.".into()); }
    let max = max_results.clamp(1, 500) as usize;
    let query_low = query.to_lowercase();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);

    let roots: Vec<PathBuf> = if let Some(r) = root {
        check_path_safety(r)?;
        let p = PathBuf::from(r);
        if !p.exists() { return Err(format!("Search root does not exist: {r}")); }
        vec![p]
    } else {
        default_roots()
    };

    let mut results: Vec<SearchMatch> = Vec::new();
    let mut seen_paths = std::collections::HashSet::<String>::new();
    let mut seen_dirs  = std::collections::HashSet::<String>::new();
    let mut dirs_visited: usize = 0;
    let mut entries_scanned: usize = 0;

    // BFS: (directory, depth). Shallow dirs are processed before deep ones,
    // so root-level matches are always found before max_results fills up.
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = std::collections::VecDeque::new();
    for root_path in roots {
        let key = root_path.to_string_lossy().to_lowercase();
        if seen_dirs.insert(key) {
            queue.push_back((root_path, 0));
        }
    }

    'bfs: while let Some((dir, depth)) = queue.pop_front() {
        if results.len() >= max || std::time::Instant::now() > deadline { break; }
        dirs_visited += 1;
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd {
            if results.len() >= max || std::time::Instant::now() > deadline { break 'bfs; }
            let Ok(entry) = entry else { continue };
            entries_scanned += 1;
            let Ok(meta) = entry.metadata() else { continue };
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_path = entry.path();
            let path_str = entry_path.to_string_lossy().into_owned();

            if name.to_lowercase().contains(&query_low) && seen_paths.insert(path_str.clone()) {
                results.push(SearchMatch {
                    path: path_str.clone(),
                    is_directory: meta.is_dir(),
                    modified_at: age_string(&meta),
                    depth,
                });
            }

            if meta.is_dir() && !should_skip(&name) {
                let dir_key = path_str.to_lowercase();
                if seen_dirs.insert(dir_key) {
                    queue.push_back((entry_path, depth + 1));
                }
            }
        }
    }

    log::info!(
        "[search_filesystem] query={:?} root={:?} dirs_visited={} entries_scanned={} results={}",
        query, root, dirs_visited, entries_scanned, results.len()
    );

    // Shallow matches first, then alphabetical within the same depth
    results.sort_by(|a, b| {
        a.depth.cmp(&b.depth)
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });

    Ok(results)
}

// ─── read_file ────────────────────────────────────────────────────────────────

const DEFAULT_MAX_BYTES: u32 = 100 * 1024;
const HARD_MAX_BYTES: u32 = 1024 * 1024;

pub fn read_file(path: &str, max_bytes: u32) -> Result<String, String> {
    check_path_safety(path)?;
    let p = Path::new(path);
    if !p.exists() { return Err(format!("File does not exist: {path}")); }
    if !p.is_file() { return Err(format!("Not a file: {path}")); }

    let limit = max_bytes.clamp(1, HARD_MAX_BYTES) as usize;
    let bytes = std::fs::read(p).map_err(|e| format!("Cannot read file: {e}"))?;
    let slice = if bytes.len() > limit { &bytes[..limit] } else { &bytes };

    match std::str::from_utf8(slice) {
        Ok(s) => {
            if bytes.len() > limit {
                Ok(format!("{s}\n\n[truncated — showed {limit} of {} bytes]", bytes.len()))
            } else {
                Ok(s.to_string())
            }
        }
        Err(_) => Err("File appears to be binary and cannot be read as text.".into()),
    }
}

// ─── get_path_info ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PathInfo {
    pub exists: bool,
    pub is_directory: bool,
    pub is_file: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_path: Option<String>,
}

pub fn get_path_info(path: &str) -> Result<PathInfo, String> {
    check_path_safety(path)?;
    let p = Path::new(path);
    let parent = p.parent().map(|pp| pp.to_string_lossy().into_owned());
    if !p.exists() {
        return Ok(PathInfo {
            exists: false, is_directory: false, is_file: false,
            size_bytes: None, modified_at: None, parent_path: parent,
        });
    }
    let meta = std::fs::metadata(p).map_err(|e| format!("Cannot stat path: {e}"))?;
    Ok(PathInfo {
        exists: true,
        is_directory: meta.is_dir(),
        is_file: meta.is_file(),
        size_bytes: meta.is_file().then(|| meta.len()),
        modified_at: Some(age_string(&meta)),
        parent_path: parent,
    })
}

// ─── Exported constants ───────────────────────────────────────────────────────

pub const DEFAULT_READ_BYTES: u32 = DEFAULT_MAX_BYTES;

// ─── Write-safety guard ───────────────────────────────────────────────────────

/// Extends check_path_safety with write-specific blocks (OS dirs, Aria project, Recycle Bin…).
pub fn check_write_safety(path: &str) -> Result<(), String> {
    check_path_safety(path)?;

    let raw = path.to_lowercase().replace('/', "\\");
    let norm = raw.trim_end_matches('\\');

    // Hard-coded protected trees
    const PROTECTED: &[&str] = &[
        "c:\\windows",
        "c:\\program files",
        "c:\\program files (x86)",
        "c:\\programdata",
        "d:\\personal-dev\\aria-v2",
    ];
    for p in PROTECTED {
        if norm == *p || norm.starts_with(&format!("{p}\\")) {
            return Err(format!(
                "Refusing to operate on protected path: {path}. This is a safety guard."
            ));
        }
    }

    // Recycle Bin on any drive
    if norm.contains("$recycle.bin") {
        return Err(format!(
            "Refusing to operate on protected path: {path}. This is a safety guard."
        ));
    }

    // Windows credential stores
    if norm.contains("appdata\\local\\microsoft\\credentials")
        || norm.contains("appdata\\roaming\\microsoft\\credentials")
        || norm.contains("appdata\\local\\microsoft\\vault")
    {
        return Err(format!(
            "Refusing to operate on protected path: {path}. This is a safety guard."
        ));
    }

    Ok(())
}

// ─── create_directory ─────────────────────────────────────────────────────────

pub fn create_directory(path: &str) -> Result<String, String> {
    check_write_safety(path)?;
    let p = Path::new(path);
    if p.is_file() {
        return Err(format!("Path already exists as a file: {path}"));
    }
    if p.is_dir() {
        return Err(format!("Directory already exists: {path}"));
    }
    std::fs::create_dir_all(p).map_err(|e| format!("Failed to create directory: {e}"))?;
    Ok(p.to_string_lossy().into_owned())
}

// ─── write_file ───────────────────────────────────────────────────────────────

pub fn write_file(path: &str, content: &str, overwrite: bool) -> Result<String, String> {
    check_write_safety(path)?;
    let p = Path::new(path);
    if p.exists() && !overwrite {
        return Err(format!(
            "File already exists: {path}. Pass overwrite=true to replace it."
        ));
    }
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent directories: {e}"))?;
        }
    }
    let bytes = content.as_bytes();
    std::fs::write(p, bytes).map_err(|e| format!("Failed to write file: {e}"))?;
    Ok(format!("{} ({} bytes written)", p.to_string_lossy(), bytes.len()))
}

// ─── move_path ────────────────────────────────────────────────────────────────

pub fn move_path(from: &str, to: &str) -> Result<String, String> {
    check_write_safety(from)?;
    check_write_safety(to)?;
    let src = Path::new(from);
    let dst = Path::new(to);
    if !src.exists() { return Err(format!("Source does not exist: {from}")); }
    if dst.exists()  { return Err(format!("Destination already exists: {to}. Choose a different path.")); }
    // Try fast same-device rename first; fall back to copy+delete for cross-device moves.
    match std::fs::rename(src, dst) {
        Ok(())                          => return Ok(format!("{from} → {to}")),
        Err(ref e) if is_xdev(e)        => { /* fall through */ }
        Err(e)                          => return Err(format!("Failed to move: {e}")),
    }
    if src.is_file() {
        std::fs::copy(src, dst).map_err(|e| format!("Copy failed during move: {e}"))?;
        std::fs::remove_file(src).map_err(|e| format!("Copied but failed to remove source: {e}"))?;
    } else {
        copy_dir_all(src, dst)?;
        std::fs::remove_dir_all(src)
            .map_err(|e| format!("Copied but failed to remove source dir: {e}"))?;
    }
    Ok(format!("{from} → {to}"))
}

fn is_xdev(e: &std::io::Error) -> bool {
    // Windows ERROR_NOT_SAME_DEVICE=17, POSIX EXDEV=18
    matches!(e.raw_os_error(), Some(17) | Some(18))
}

// ─── copy_path ────────────────────────────────────────────────────────────────

pub fn copy_path(from: &str, to: &str) -> Result<String, String> {
    check_path_safety(from)?;   // source only needs read safety
    check_write_safety(to)?;
    let src = Path::new(from);
    let dst = Path::new(to);
    if !src.exists() { return Err(format!("Source does not exist: {from}")); }
    if dst.exists()  { return Err(format!("Destination already exists: {to}. Choose a different path.")); }
    if src.is_file() {
        if let Some(parent) = dst.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create destination parent: {e}"))?;
            }
        }
        let bytes = std::fs::copy(src, dst).map_err(|e| format!("Failed to copy: {e}"))?;
        Ok(format!("{from} → {to} ({bytes} bytes)"))
    } else if src.is_dir() {
        copy_dir_all(src, dst)?;
        Ok(format!("{from} → {to} (directory copied)"))
    } else {
        Err(format!("Not a file or directory: {from}"))
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("Failed to create destination directory: {e}"))?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| format!("Cannot read source directory: {e}"))?
    {
        let Ok(entry) = entry else { continue };
        let src_child = entry.path();
        let dst_child = dst.join(entry.file_name());
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            copy_dir_all(&src_child, &dst_child)?;
        } else {
            std::fs::copy(&src_child, &dst_child)
                .map_err(|e| format!("Failed to copy {}: {e}", src_child.display()))?;
        }
    }
    Ok(())
}

// ─── delete_path (Recycle Bin) ────────────────────────────────────────────────

pub fn delete_path(path: &str) -> Result<String, String> {
    check_write_safety(path)?;
    let p = Path::new(path);
    if !p.exists() { return Err(format!("Path does not exist: {path}")); }
    trash::delete(p).map_err(|e| format!("Failed to move to Recycle Bin: {e}"))?;
    Ok(format!("{path} moved to Recycle Bin (recoverable)."))
}

// ─── open_in_app ──────────────────────────────────────────────────────────────

pub const APP_WHITELIST: &[(&str, &str)] = &[
    ("vscode",   "code"),
    ("explorer", "explorer.exe"),
    ("chrome",   "chrome.exe"),
    ("notepad",  "notepad.exe"),
];

fn resolve_app_exe(app_name: &str) -> Option<std::ffi::OsString> {
    if app_name == "vscode" {
        // VS Code installs per-user on Windows; try the known path before relying on PATH.
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let candidate = PathBuf::from(local)
                .join("Programs")
                .join("Microsoft VS Code")
                .join("Code.exe");
            if candidate.exists() {
                return Some(candidate.into_os_string());
            }
        }
    }
    None
}

pub fn open_in_app(path: &str, app: Option<&str>) -> Result<String, String> {
    check_path_safety(path)?;
    let p = Path::new(path);
    if !p.exists() { return Err(format!("Path does not exist: {path}")); }
    match app {
        None => {
            // Default Windows handler via explorer.exe
            std::process::Command::new("explorer.exe")
                .arg(path)
                .spawn()
                .map_err(|e| format!("Failed to open: {e}"))?;
            Ok(format!("Opened {path} with default application."))
        }
        Some(app_name) => {
            let cmd_str = APP_WHITELIST.iter()
                .find(|(key, _)| *key == app_name)
                .map(|(_, exe)| *exe)
                .ok_or_else(|| {
                    let allowed = APP_WHITELIST.iter()
                        .map(|(k, _)| *k).collect::<Vec<_>>().join(", ");
                    format!("Unknown app: {app_name:?}. Allowed: {allowed}")
                })?;
            // Prefer the resolved absolute path (e.g. per-user VS Code install) over PATH lookup.
            let exe: std::ffi::OsString = resolve_app_exe(app_name)
                .unwrap_or_else(|| cmd_str.into());
            std::process::Command::new(&exe)
                .arg(path)
                .spawn()
                .map_err(|e| format!("Failed to open with {app_name}: {e}"))?;
            Ok(format!("Opened {path} with {app_name}."))
        }
    }
}

// ─── run_command ──────────────────────────────────────────────────────────────

/// Each entry: (command_name, argv). Argv[0] is the executable, rest are args.
pub const COMMAND_WHITELIST: &[(&str, &[&str])] = &[
    ("open_aria_project",    &["code", "D:\\personal-dev\\aria-v2"]),
    ("open_personal_folder", &["explorer", "D:\\Personal"]),
];

pub fn run_command(name: &str) -> Result<String, String> {
    let (_, parts) = COMMAND_WHITELIST.iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| {
            let allowed = COMMAND_WHITELIST.iter()
                .map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
            format!("Unknown command: {name:?}. Allowed: {allowed}")
        })?;
    let (exe, args) = parts.split_first()
        .expect("whitelist entry must have at least one element");
    std::process::Command::new(exe)
        .args(args)
        .spawn()
        .map_err(|e| format!("Failed to run {name}: {e}"))?;
    Ok(format!("Launched: {name}"))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bfs_finds_root_level_match() {
        // Requires D:\Personal to exist on this machine.
        let results = search_filesystem("personal", Some("D:\\"), 100)
            .expect("search should succeed");
        let found = results.iter().any(|m| {
            m.path.to_lowercase() == "d:\\personal"
        });
        assert!(
            found,
            "Expected D:\\Personal in results.\nGot: {:?}",
            results.iter().map(|m| &m.path).collect::<Vec<_>>()
        );
    }
}
