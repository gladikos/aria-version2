// ─── Windows implementation ───────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod imp {
    use std::path::{Path, PathBuf};

    // Maps canonical lowercase names → a command Windows can resolve via App Paths or PATH.
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

    fn spawn_cmd(cmd: &str, args: &[String]) -> bool {
        let mut c = std::process::Command::new(cmd);
        c.args(args);
        crate::process_utils::no_window(&mut c);
        match c.spawn() {
            Ok(_)  => { log::info!("[launch_app] spawned {:?} ({} arg(s))", cmd, args.len()); true }
            Err(e) => { log::debug!("[launch_app] command {:?} failed: {}", cmd, e); false }
        }
    }

    // Fire-and-forget: open a path (exe or .lnk), with optional args.
    // .lnk files: uses `cmd /c start "" <path> <args>` (shell resolves the shortcut).
    fn spawn_path(path: &Path, args: &[String]) -> bool {
        let p = path.to_string_lossy().into_owned();
        let is_lnk = path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false);

        if is_lnk || args.is_empty() {
            let mut c = std::process::Command::new("cmd");
            c.arg("/c").arg("start").arg("").arg(&p).args(args);
            crate::process_utils::no_window(&mut c);
            match c.spawn() {
                Ok(_)  => { log::info!("[launch_app] opened {:?} via cmd start", p); true }
                Err(e) => { log::debug!("[launch_app] start {:?} failed: {}", p, e); false }
            }
        } else {
            let mut c = std::process::Command::new(&p);
            c.args(args);
            crate::process_utils::no_window(&mut c);
            match c.spawn() {
                Ok(_)  => { log::info!("[launch_app] spawned {:?} directly ({} arg(s))", p, args.len()); true }
                Err(e) => {
                    log::debug!("[launch_app] direct spawn failed ({}), retrying via cmd start", e);
                    let mut c2 = std::process::Command::new("cmd");
                    c2.arg("/c").arg("start").arg("").arg(&p).args(args);
                    crate::process_utils::no_window(&mut c2);
                    match c2.spawn() {
                        Ok(_)  => { log::info!("[launch_app] opened {:?} via cmd start fallback", p); true }
                        Err(e2) => { log::debug!("[launch_app] cmd start {:?} also failed: {}", p, e2); false }
                    }
                }
            }
        }
    }

    // ─── Strategy 2: Start Menu .lnk search ──────────────────────────────────

    fn start_menu_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        dirs.push(PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs"));
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(format!(r"{appdata}\Microsoft\Windows\Start Menu\Programs")));
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

    // ─── Strategy 3: Registry App Paths ──────────────────────────────────────

    fn find_registry_path(query: &str) -> Option<PathBuf> {
        use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let key_path = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{query}.exe");
        let key = hklm.open_subkey(&key_path).ok()?;
        let raw: String = key.get_value("").ok()?;
        let p = PathBuf::from(raw.trim_matches('"'));
        if p.exists() { Some(p) } else { None }
    }

    // ─── Strategy 4: Install-dir filesystem search ────────────────────────────

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

    // ─── Public entry point ───────────────────────────────────────────────────

    pub fn launch_app(name: &str, args: &[String]) -> Result<String, String> {
        let query = name.trim().to_lowercase();
        log::info!("[launch_app] resolving {:?} ({} extra arg(s))", name, args.len());

        let mut alias_fallback: Option<String> = None;
        if let Some((_, alias)) = ALIASES.iter().find(|(k, _)| *k == query) {
            log::info!("[launch_app] strategy 1: {:?} → {:?}", query, alias);
            if spawn_cmd(alias, args) {
                return Ok(format!("Opened {name}."));
            }
            log::warn!("[launch_app] strategy 1 failed for {:?}, trying further", alias);
            if *alias != query.as_str() {
                alias_fallback = Some(alias.to_string());
            }
        }

        let mut candidates: Vec<&str> = vec![query.as_str()];
        if let Some(ref a) = alias_fallback { candidates.push(a.as_str()); }

        for q in candidates {
            if let Some(lnk) = find_start_menu_lnk(q) {
                log::info!("[launch_app] strategy 2 (q={:?}): {:?}", q, lnk);
                if spawn_path(&lnk, args) { return Ok(format!("Opened {name}.")); }
            }
            if let Some(exe) = find_registry_path(q) {
                log::info!("[launch_app] strategy 3 (q={:?}): {:?}", q, exe);
                if spawn_path(&exe, args) { return Ok(format!("Opened {name}.")); }
            }
            if let Some(exe) = find_in_install_dirs(q) {
                log::info!("[launch_app] strategy 4 (q={:?}): {:?}", q, exe);
                if spawn_path(&exe, args) { return Ok(format!("Opened {name}.")); }
            }
        }

        log::warn!("[launch_app] all strategies failed for {:?}", name);
        Err(format!(
            "Couldn't find an app called '{name}'. It might not be installed, \
             or it's installed somewhere unusual. Check the Start Menu — if it's there, \
             tell me the exact name shown."
        ))
    }
}

// ─── macOS stub ───────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod imp {
    pub fn launch_app(name: &str, args: &[String]) -> Result<String, String> {
        // TODO(mac): implement using `open -a <AppName>` shell-out.
        // The alias table needs Mac equivalents and the strategies need rewriting for macOS.
        let _ = (name, args);
        Err(format!(
            "TODO(mac): launch_app not yet implemented on macOS. \
             Would use: open -a \"{name}\""
        ))
    }
}

// ─── Fallback for other platforms ─────────────────────────────────────────────

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod imp {
    pub fn launch_app(name: &str, args: &[String]) -> Result<String, String> {
        let _ = (name, args);
        Err("launch_app not implemented for this OS".to_string())
    }
}

// ─── Public API ───────────────────────────────────────────────────────────────

pub fn launch_app(name: &str, args: &[String]) -> Result<String, String> {
    imp::launch_app(name, args)
}
