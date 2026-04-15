use std::path::PathBuf;

pub struct ShellContext {
    pub cwd: PathBuf,
    pub user: String,
    /// Last N commands from shell history (most recent last).
    pub recent_history: Vec<String>,
    /// Interesting executables visible on $PATH.
    pub installed_tools: Vec<String>,
}

impl ShellContext {
    pub fn current() -> Self {
        ShellContext {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            user: std::env::var("USER")
                .or_else(|_| std::env::var("LOGNAME"))
                .unwrap_or_else(|_| "user".into()),
            recent_history: read_history(20),
            installed_tools: installed_tools(),
        }
    }
}

/// Read the last `n` entries from zsh history via `fc -l`.
fn read_history(n: usize) -> Vec<String> {
    let output = std::process::Command::new("zsh")
        .args(["-i", "-c", &format!("fc -l -{n}")])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    let Ok(out) = output else { return vec![] };

    String::from_utf8_lossy(&out.stdout)
        .lines()
        // fc -l output: "  123  command text" — strip the leading number
        .filter_map(|line| {
            let trimmed = line.trim();
            // Skip the number prefix (digits + whitespace)
            let cmd = trimmed
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start();
            if cmd.is_empty() {
                None
            } else {
                Some(cmd.to_string())
            }
        })
        .collect()
}

/// Return names of interesting executables found on $PATH.
/// Filters to a curated set of tool families relevant to shell use.
fn installed_tools() -> Vec<String> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let dirs: Vec<&str> = path_var.split(':').collect();

    // Prefixes/names worth surfacing to the model.
    const INTERESTING: &[&str] = &[
        "git", "cargo", "rustc", "swift", "python", "python3", "node", "npm",
        "yarn", "pnpm", "bun", "deno", "docker", "kubectl", "helm", "terraform",
        "aws", "gcloud", "az", "brew", "port", "claude", "gh", "jq", "yq",
        "ffmpeg", "convert", "curl", "wget", "rsync", "tmux", "screen",
        "nvim", "vim", "emacs", "code", "supabase", "fly", "vercel",
    ];

    let mut found: Vec<String> = Vec::new();
    for name in INTERESTING {
        for dir in &dirs {
            let p = std::path::Path::new(dir).join(name);
            if p.exists() {
                found.push(name.to_string());
                break;
            }
        }
    }
    found
}

pub fn build_prompt(request: &str, ctx: &ShellContext) -> String {
    let history_section = if ctx.recent_history.is_empty() {
        String::new()
    } else {
        format!(
            "Recent commands (most recent last):\n{}\n\n",
            ctx.recent_history
                .iter()
                .map(|c| format!("  {c}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

    let tools_section = if ctx.installed_tools.is_empty() {
        String::new()
    } else {
        format!("Installed tools: {}\n\n", ctx.installed_tools.join(", "))
    };

    format!(
        "You are a shell command translator. The user is in a zsh session on macOS.\n\
         Current directory: {cwd}\n\
         User: {user}\n\n\
         {tools_section}\
         {history_section}\
         Rules:\n\
         - Respond with ONLY a single shell command. No explanation. No markdown. No alternatives.\n\
         - Do not wrap the command in backticks or code fences.\n\
         - Use macOS-compatible commands (e.g. ifconfig not ip, stat -f not stat -c, BSD find syntax).\n\
         - Match the style of the user's recent commands where relevant.\n\
         - If the request is ambiguous, prefer the safest interpretation.\n\
         - If the request cannot be translated to a single shell command, output exactly: CANNOT_TRANSLATE\n\n\
         Examples:\n\
         - \"compress this directory\" → tar -czf archive.tar.gz .\n\
         - \"list files sorted by size\" → ls -lhS\n\
         - \"show memory usage by process\" → ps aux | sort -rk4 | head -20\n\
         - \"find files modified in the last day\" → find . -mtime -1\n\
         - \"show lines changed in git today\" → git log --since=midnight --stat\n\
         - \"count rust files\" → find . -name '*.rs' | wc -l\n\n\
         User request: {request}",
        cwd = ctx.cwd.display(),
        user = ctx.user,
        request = request,
    )
}
