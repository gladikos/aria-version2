use std::path::{Path, PathBuf};

// ─── Built-in aliases (strategy 1) ───────────────────────────────────────────
// Maps canonical lowercase names → a command Windows can resolve via App Paths
// or PATH. If spawn fails, strategies 2-4 take over.

const ALIASES: &[(&str, &str)] = &[
    // Microsoft Office
    ("word",               "winword"),
    ("ms word",            "winword"),
    ("microsoft word",     "winword"),
    ("excel",              "excel"),
    ("ms excel",           "excel"),
    ("microsoft excel",    "excel"),
    ("powerpoint",         "powerpnt"),
    ("ppt",                "powerpnt"),
    ("ms powerpoint",      "powerpnt"),
    ("outlook",            "outlook"),
    ("onenote",            "onenote"),
    ("access",             "msaccess"),
    ("publisher",          "mspub"),
    // Browsers
    ("chrome",             "chrome"),
    ("google chrome",      "chrome"),
    ("firefox",            "firefox"),
    ("edge",               "msedge"),
    ("microsoft edge",     "msedge"),
    ("brave",              "brave"),
    ("opera",              "opera"),
    // Dev tools
    ("vscode",             "code"),
    ("vs code",            "code"),
    ("visual studio code", "code"),
    ("cursor",             "cursor"),
    ("powershell",         "powershell"),
    ("cmd",                "cmd"),
    ("command prompt",     "cmd"),
    ("windows terminal",   "wt"),
    ("wsl",                "wsl"),
    // Communication
    ("discord",            "discord"),
    ("slack",              "slack"),
    ("teams",              "ms-teams"),
    ("microsoft teams",    "ms-teams"),
    ("zoom",               "zoom"),
    ("skype",              "skype"),
    ("telegram",           "telegram"),
    ("signal",             "signal"),
    // Media / entertainment
    ("spotify",            "spotify"),
    ("vlc",                "vlc"),
    ("steam",              "steam"),
    ("itunes",             "itunes"),
    // Productivity
    ("notepad",            "notepad"),
    ("notepad++",          "notepad++"),
    ("calculator",         "calc"),
    ("calc",               "calc"),
    ("explorer",           "explorer"),
    ("file explorer",      "explorer"),
    ("task manager",       "taskmgr"),
    ("paint",              "mspaint"),
    ("wordpad",            "wordpad"),
    // Gaming
    ("epic games",         "EpicGamesLauncher"),
    ("epic",               "EpicGamesLauncher"),
    ("gog galaxy",         "GalaxyClient"),
    ("gog",                "GalaxyClient"),
];

// ─── Launch primitives ────────────────────────────────────────────────────────

// Fire-and-forget: try to spawn cmd as a bare command name (resolved via PATH / App Paths registry).
fn spawn_cmd(cmd: &str) -> bool {
    match std::process::Command::new(cmd).spawn() {
        Ok(_)  => { log::info!("[launch_app] spawned command {:?}", cmd); true }
        Err(e) => { log::debug!("[launch_app] command {:?} failed: {}", cmd, e); false }
    }
}

// Fire-and-forget: open a path (exe or .lnk) via `cmd /c start "" <path>`.
fn spawn_path(path: &Path) -> bool {
    let p = path.to_string_lossy().into_owned();
    match std::process::Command::new("cmd")
        .arg("/c").arg("start").arg("").arg(&p)
        .spawn()
    {
        Ok(_)  => { log::info!("[launch_app] opened path {:?}", p); true }
        Err(e) => { log::debug!("[launch_app] start {:?} failed: {}", p, e); false }
    }
}

// ─── Strategy 2: Start Menu .lnk search ──────────────────────────────────────

fn start_menu_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.push(PathBuf::from(
        r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs",
    ));
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(format!(
            r"{appdata}\Microsoft\Windows\Start Menu\Programs"
        )));
    }
    dirs
}

fn lnk_score(stem: &str, query: &str) -> i32 {
    let s = stem.to_lowercase();
    if s == query                { 30 }
    else if s.starts_with(query) { 20 }
    else if s.contains(query)    { 10 }
    else                         { 0  }
}

fn walk_lnk(dir: &Path, query: &str, depth: usize, best: &mut Option<(i32, PathBuf)>) {
    if depth > 5 { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_lnk(&path, query, depth + 1, best);
        } else {
            let is_lnk = path.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("lnk"))
                .unwrap_or(false);
            if is_lnk {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let score = lnk_score(stem, query);
                    if score > 0 && best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                        *best = Some((score, path));
                    }
                }
            }
        }
    }
}

fn find_start_menu_lnk(query: &str) -> Option<PathBuf> {
    let mut best: Option<(i32, PathBuf)> = None;
    for dir in start_menu_dirs() {
        walk_lnk(&dir, query, 0, &mut best);
    }
    best.map(|(_, p)| p)
}

// ─── Strategy 3: Registry App Paths ──────────────────────────────────────────

fn find_registry_path(query: &str) -> Option<PathBuf> {
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key_path = format!(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{query}.exe"
    );
    let key = hklm.open_subkey(&key_path).ok()?;
    let raw: String = key.get_value("").ok()?;
    let p = PathBuf::from(raw.trim_matches('"'));
    if p.exists() { Some(p) } else { None }
}

// ─── Strategy 4: Install-dir filesystem search ───────────────────────────────

fn walk_exe(dir: &Path, target: &str, depth: usize) -> Option<PathBuf> {
    if depth > 3 { return None; }
    let Ok(entries) = std::fs::read_dir(dir) else { return None };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let matches = path.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_lowercase() == target)
                .unwrap_or(false);
            if matches { return Some(path); }
        } else if path.is_dir() {
            if let Some(found) = walk_exe(&path, target, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

fn find_in_install_dirs(query: &str) -> Option<PathBuf> {
    let target = format!("{}.exe", query);
    let mut dirs = vec![
        PathBuf::from(r"C:\Program Files"),
        PathBuf::from(r"C:\Program Files (x86)"),
    ];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        dirs.push(PathBuf::from(format!(r"{local}\Programs")));
    }
    for dir in &dirs {
        if dir.exists() {
            if let Some(found) = walk_exe(dir, &target, 0) {
                return Some(found);
            }
        }
    }
    None
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub fn launch_app(name: &str) -> Result<String, String> {
    let query = name.trim().to_lowercase();
    log::info!("[launch_app] resolving {:?}", name);

    // Strategy 1: built-in alias → direct command spawn
    let mut alias_fallback: Option<String> = None;
    if let Some((_, alias)) = ALIASES.iter().find(|(k, _)| *k == query) {
        log::info!("[launch_app] strategy 1: {:?} → {:?}", query, alias);
        if spawn_cmd(alias) {
            return Ok(format!("Opened {name}."));
        }
        log::warn!("[launch_app] strategy 1 failed for {:?}, trying further", alias);
        // Keep the alias so strategies 2-4 can also search by it (e.g. "code" for "vs code").
        if *alias != query.as_str() {
            alias_fallback = Some(alias.to_string());
        }
    }

    // Strategies 2-4: search with the original query, then with the alias if different.
    // This lets "vs code" → alias "code" → find Code.exe in %LOCALAPPDATA%\Programs.
    let mut candidates: Vec<&str> = vec![query.as_str()];
    if let Some(ref a) = alias_fallback { candidates.push(a.as_str()); }

    for q in candidates {
        // Strategy 2: Start Menu .lnk search
        if let Some(lnk) = find_start_menu_lnk(q) {
            log::info!("[launch_app] strategy 2 (q={:?}): {:?}", q, lnk);
            if spawn_path(&lnk) {
                return Ok(format!("Opened {name}."));
            }
        }

        // Strategy 3: Windows App Paths registry
        if let Some(exe) = find_registry_path(q) {
            log::info!("[launch_app] strategy 3 (q={:?}): {:?}", q, exe);
            if spawn_path(&exe) {
                return Ok(format!("Opened {name}."));
            }
        }

        // Strategy 4: filesystem search in Program Files / LocalAppData
        if let Some(exe) = find_in_install_dirs(q) {
            log::info!("[launch_app] strategy 4 (q={:?}): {:?}", q, exe);
            if spawn_path(&exe) {
                return Ok(format!("Opened {name}."));
            }
        }
    }

    log::warn!("[launch_app] all strategies failed for {:?}", name);
    Err(format!(
        "Couldn't find an app called '{name}'. It might not be installed, \
         or it's installed somewhere unusual. Check the Start Menu — if it's there, \
         tell me the exact name shown."
    ))
}
