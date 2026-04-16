use anyhow::Result;
use std::io::Write;

use crate::config::{Backend, NlshConfig};

// ── ANSI color constants ─────────────────────────────────────────────────────
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

// ── Key abstraction ──────────────────────────────────────────────────────────

#[derive(Debug)]
enum Key {
    Char(char),
    Enter,
    Escape,
    Up,
    Down,
}

/// Read one logical keypress from stdin (fd 0).
/// Blocks until a byte is available, then detects arrow-key escape sequences
/// by briefly switching fd 0 to O_NONBLOCK to consume the remaining bytes.
fn read_key() -> Option<Key> {
    let mut buf = [0u8; 1];
    loop {
        let n = unsafe { libc::read(0, buf.as_mut_ptr() as *mut libc::c_void, 1) };
        if n == 1 {
            break;
        }
        if n == 0 {
            return None;
        }
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return None;
    }

    match buf[0] {
        0x0d | 0x0a => Some(Key::Enter),
        0x1b => {
            // Try to read the rest of an escape sequence non-blocking.
            let saved = unsafe { libc::fcntl(0, libc::F_GETFL) };
            unsafe { libc::fcntl(0, libc::F_SETFL, saved | libc::O_NONBLOCK) };
            let mut seq = [0u8; 2];
            let n = unsafe { libc::read(0, seq.as_mut_ptr() as *mut libc::c_void, 2) };
            unsafe { libc::fcntl(0, libc::F_SETFL, saved) };

            if n == 2 && seq[0] == b'[' {
                match seq[1] {
                    b'A' => Some(Key::Up),
                    b'B' => Some(Key::Down),
                    _ => Some(Key::Escape),
                }
            } else {
                Some(Key::Escape)
            }
        }
        // q / Q / Ctrl+C → treat as quit
        0x03 | b'q' | b'Q' => Some(Key::Char('q')),
        b if b.is_ascii_graphic() => Some(Key::Char(b as char)),
        _ => None,
    }
}

// ── Screen helpers ───────────────────────────────────────────────────────────

/// Clear screen and home cursor (works in raw mode).
fn clear() {
    print!("\x1b[2J\x1b[H");
    std::io::stdout().flush().ok();
}

/// Print a line terminated with CR+LF (required in raw mode).
macro_rules! tln {
    () => { print!("\r\n") };
    ($($arg:tt)*) => { print!("{}\r\n", format!($($arg)*)) };
}

fn draw_header() {
    tln!("  {BOLD}╭────────────────────────────────────────────╮{RESET}");
    tln!("  {BOLD}│{CYAN}         nlsh · model setup                 {RESET}{BOLD}│{RESET}");
    tln!("  {BOLD}╰────────────────────────────────────────────╯{RESET}");
    tln!();
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Run the interactive model-selection TUI.
///
/// Enters raw mode, guides the user through backend and model selection,
/// optionally pulls an Ollama model, saves the config, then returns it.
/// Returns `Err` if the user cancels or a fatal error occurs.
pub fn run_setup() -> Result<NlshConfig> {
    let _raw = crate::terminal::RawMode::enter()?;

    let apple_ok = crate::llm::check_apple_shim();
    let backend = select_backend(apple_ok)?;

    let mut config = NlshConfig::default();

    match backend {
        Backend::Apple => {
            config.backend = Backend::Apple;
            config.save()?;
        }

        Backend::Ollama => {
            ensure_ollama_installed()?;

            let model = select_model()?;
            config.backend = Backend::Ollama;
            config.ollama_model = model.clone();
            config.ollama_url = "http://localhost:11434".to_string();

            ensure_ollama_running(&config.ollama_url)?;

            if !crate::ollama::has_model(&config.ollama_url, &model) {
                pull_with_progress(&config.ollama_url, &model)?;
            }

            config.save()?;
        }
    }

    offer_default_shell()?;

    Ok(config)
}

// ── Screen 1: backend selection ──────────────────────────────────────────────

fn select_backend(apple_ok: bool) -> Result<Backend> {
    // If Apple Intelligence is unavailable, default cursor to Ollama.
    let mut sel = if apple_ok { 0usize } else { 1usize };

    loop {
        clear();
        draw_header();
        tln!("  Choose an AI backend:");
        tln!();

        let items: &[(&str, &str)] = &[
            ("Apple Intelligence", "on-device, no download"),
            ("Ollama local model", "~986 MB download"),
        ];

        for (i, (name, desc)) in items.iter().enumerate() {
            let cursor = if i == sel {
                format!("{CYAN}▶{RESET}")
            } else {
                " ".to_string()
            };

            if i == 0 && !apple_ok {
                tln!(
                    "  {cursor} {DIM}{n}  {name}  {desc}  (unavailable on this device){RESET}",
                    n = i + 1
                );
            } else {
                let label = if i == sel {
                    format!("{BOLD}{name}{RESET}")
                } else {
                    name.to_string()
                };
                tln!("  {cursor} {n}  {label}  {DIM}{desc}{RESET}", n = i + 1);
            }
        }

        tln!();
        tln!("  {DIM}[↑↓ / 1-2] navigate   [Enter] confirm   [q] quit{RESET}");
        std::io::stdout().flush().ok();

        match read_key() {
            Some(Key::Up) => {
                sel = if sel == 0 { items.len() - 1 } else { sel - 1 };
                // Skip Apple option if unavailable
                if sel == 0 && !apple_ok {
                    sel = items.len() - 1;
                }
            }
            Some(Key::Down) => {
                sel = (sel + 1) % items.len();
                if sel == 0 && !apple_ok {
                    sel = 1;
                }
            }
            Some(Key::Char('1')) => {
                if apple_ok {
                    return Ok(Backend::Apple);
                }
            }
            Some(Key::Char('2')) => return Ok(Backend::Ollama),
            Some(Key::Enter) => {
                return Ok(if sel == 0 {
                    Backend::Apple
                } else {
                    Backend::Ollama
                });
            }
            Some(Key::Char('q')) | Some(Key::Escape) => {
                clear();
                return Err(anyhow::anyhow!("setup cancelled"));
            }
            _ => {}
        }
    }
}

// ── Ollama install / start checks ────────────────────────────────────────────

fn ollama_in_path() -> bool {
    std::process::Command::new("which")
        .arg("ollama")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn brew_in_path() -> bool {
    std::process::Command::new("which")
        .arg("brew")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ensure_ollama_installed() -> Result<()> {
    if ollama_in_path() {
        return Ok(());
    }

    // ── No Homebrew: show manual install instructions ─────────────────────
    if !brew_in_path() {
        clear();
        draw_header();
        tln!("  {YELLOW}Ollama is not installed.{RESET}");
        tln!();
        tln!("  Install it from: {BOLD}https://ollama.com/download{RESET}");
        tln!("  Then run: {CYAN}nlsh --setup{RESET}");
        tln!();
        tln!("  (Or install Homebrew at {DIM}brew.sh{RESET} for automatic install.)");
        tln!();
        tln!("  {DIM}[any key] exit{RESET}");
        std::io::stdout().flush().ok();
        read_key();
        return Err(anyhow::anyhow!("Ollama not installed"));
    }

    // ── Homebrew available: install silently with spinner ─────────────────
    clear();
    draw_header();
    tln!("  Ollama not found — installing via Homebrew...");
    tln!();
    std::io::stdout().flush().ok();

    let mut child = std::process::Command::new("brew")
        .args(["install", "--cask", "ollama"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to run brew: {e}"))?;

    let spinners = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let mut tick = 0usize;

    loop {
        match child.try_wait()? {
            Some(status) if status.success() => {
                print!("\r\x1b[2K");
                tln!("  {GREEN}✓{RESET}  Ollama installed.");
                tln!();
                std::io::stdout().flush().ok();

                // Homebrew installs the app but may not add the CLI to PATH yet.
                // The cask puts the binary at /usr/local/bin/ollama or
                // /opt/homebrew/bin/ollama — check both before giving up.
                if !ollama_in_path() {
                    // Add Homebrew prefix to PATH for subsequent commands.
                    let homebrew_bin = if std::path::Path::new("/opt/homebrew/bin/ollama").exists() {
                        "/opt/homebrew/bin"
                    } else {
                        "/usr/local/bin"
                    };
                    let current_path = std::env::var("PATH").unwrap_or_default();
                    std::env::set_var("PATH", format!("{homebrew_bin}:{current_path}"));
                }
                return Ok(());
            }
            Some(_) => {
                print!("\r\x1b[2K");
                tln!("  {RED}Homebrew install failed.{RESET}");
                tln!();
                tln!("  Try manually: {BOLD}https://ollama.com/download{RESET}");
                tln!("  Then run: {CYAN}nlsh --setup{RESET}");
                tln!();
                tln!("  {DIM}[any key] exit{RESET}");
                std::io::stdout().flush().ok();
                read_key();
                return Err(anyhow::anyhow!("Homebrew install failed"));
            }
            None => {
                print!(
                    "\r\x1b[2K  {CYAN}{sp}{RESET}  installing Ollama via Homebrew...",
                    sp = spinners[tick % spinners.len()]
                );
                std::io::stdout().flush().ok();
                tick = tick.wrapping_add(1);
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        }
    }
}

fn ensure_ollama_running(url: &str) -> Result<()> {
    if crate::ollama::is_running(url) {
        return Ok(());
    }

    clear();
    draw_header();
    print!("  Starting Ollama...\r\n");
    std::io::stdout().flush().ok();

    // Attempt to launch `ollama serve` in the background.
    let _ = std::process::Command::new("ollama")
        .arg("serve")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    // Poll up to 5 s (10 × 500 ms).
    let spinners = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    for i in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if crate::ollama::is_running(url) {
            return Ok(());
        }
        print!(
            "\r\x1b[2K  {CYAN}{sp}{RESET}  waiting for Ollama...",
            sp = spinners[i % spinners.len()]
        );
        std::io::stdout().flush().ok();
    }

    clear();
    draw_header();
    tln!("  {RED}Could not start Ollama.{RESET}");
    tln!();
    tln!("  Start it with: {CYAN}ollama serve{RESET}");
    tln!("  Then run:      {CYAN}nlsh --setup{RESET}");
    tln!();
    tln!("  {DIM}[any key] exit{RESET}");
    std::io::stdout().flush().ok();
    read_key();

    Err(anyhow::anyhow!("Ollama not running"))
}

// ── Screen 2: model selection ─────────────────────────────────────────────────

struct ModelOption {
    tag: &'static str,
    size: &'static str,
    note: &'static str,
}

fn select_model() -> Result<String> {
    let models = [
        ModelOption {
            tag: "qwen2.5-coder:1.5b",
            size: "~986 MB",
            note: "recommended",
        },
        ModelOption {
            tag: "qwen2.5-coder:3b",
            size: "~2.0 GB",
            note: "higher quality",
        },
    ];

    let mut sel = 0usize;

    loop {
        clear();
        draw_header();
        tln!("  Select model:");
        tln!();

        for (i, m) in models.iter().enumerate() {
            let cursor = if i == sel {
                format!("{CYAN}▶{RESET}")
            } else {
                " ".to_string()
            };
            let label = if i == sel {
                format!("{BOLD}{}{RESET}", m.tag)
            } else {
                m.tag.to_string()
            };
            let note_color = if i == 0 { GREEN } else { DIM };
            tln!(
                "  {cursor} {n}  {label}  {DIM}{sz}{RESET}  {note_color}{note}{RESET}",
                n = i + 1,
                sz = m.size,
                note = m.note
            );
        }

        tln!();
        tln!("  {DIM}[↑↓ / 1-2] navigate   [Enter] confirm   [Esc] back{RESET}");
        std::io::stdout().flush().ok();

        match read_key() {
            Some(Key::Up) => {
                sel = if sel == 0 { models.len() - 1 } else { sel - 1 };
            }
            Some(Key::Down) => {
                sel = (sel + 1) % models.len();
            }
            Some(Key::Char('1')) => return Ok(models[0].tag.to_string()),
            Some(Key::Char('2')) => return Ok(models[1].tag.to_string()),
            Some(Key::Enter) => return Ok(models[sel].tag.to_string()),
            Some(Key::Escape) | Some(Key::Char('q')) => {
                return Err(anyhow::anyhow!("setup cancelled"));
            }
            _ => {}
        }
    }
}

// ── Screen 3: pull progress ──────────────────────────────────────────────────

fn pull_with_progress(url: &str, model: &str) -> Result<()> {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };

    clear();
    draw_header();
    tln!("  Pulling {BOLD}{model}{RESET}...");
    tln!();
    std::io::stdout().flush().ok();

    let progress: Arc<Mutex<(u64, u64)>> = Arc::new(Mutex::new((0, 0)));
    let done: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let progress_bg = progress.clone();
    let done_bg = done.clone();
    let url_bg = url.to_string();
    let model_bg = model.to_string();

    let handle = std::thread::spawn(move || {
        let result = crate::ollama::pull_model(&url_bg, &model_bg, move |c, t| {
            if let Ok(mut p) = progress_bg.lock() {
                *p = (c, t);
            }
        });
        done_bg.store(true, Ordering::Relaxed);
        result
    });

    let mut tick: usize = 0;
    let spinners = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    loop {
        let (completed, total) = *progress.lock().unwrap();

        if total > 0 {
            let frac = completed as f64 / total as f64;
            let filled = (frac * 28.0) as usize;
            let empty = 28usize.saturating_sub(filled);
            let pct = (frac * 100.0) as u64;

            print!(
                "\r\x1b[2K  {CYAN}{fill}{dim_empty}{RESET}  {BOLD}{pct:3}%{RESET}  {DIM}{done} / {tot}{RESET}",
                fill = "█".repeat(filled),
                dim_empty = format!("{DIM}{}{RESET}", "░".repeat(empty)),
                done = fmt_bytes(completed),
                tot = fmt_bytes(total),
            );
        } else {
            print!(
                "\r\x1b[2K  {CYAN}{sp}{RESET}  pulling layers...",
                sp = spinners[tick % spinners.len()]
            );
        }
        std::io::stdout().flush().ok();
        tick = tick.wrapping_add(1);

        if done.load(Ordering::Relaxed) {
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(120));
    }

    let result = handle
        .join()
        .map_err(|_| anyhow::anyhow!("pull thread panicked"))?;

    if let Err(e) = result {
        tln!();
        tln!("  {RED}Pull failed: {e}{RESET}");
        tln!();
        tln!("  {DIM}[any key] exit{RESET}");
        std::io::stdout().flush().ok();
        read_key();
        return Err(e);
    }

    tln!();
    tln!();
    tln!("  {GREEN}✓{RESET}  {BOLD}{model}{RESET} downloaded.");
    tln!();
    tln!("  {DIM}[Enter] continue{RESET}");
    std::io::stdout().flush().ok();

    loop {
        match read_key() {
            Some(Key::Enter) | Some(Key::Char(_)) => break,
            _ => {}
        }
    }

    Ok(())
}

// ── Default shell offer ───────────────────────────────────────────────────────

fn offer_default_shell() -> Result<()> {
    const NLSH_BIN: &str = "/usr/local/bin/nlsh";

    // Only offer if the installed binary exists (i.e. user ran the .pkg installer
    // or `nlsh --install`). Not applicable for `cargo run` dev sessions.
    if !std::path::Path::new(NLSH_BIN).exists() {
        clear();
        draw_header();
        tln!("  {GREEN}✓{RESET}  Setup complete.");
        tln!();
        tln!("  Starting nlsh...");
        std::io::stdout().flush().ok();
        std::thread::sleep(std::time::Duration::from_millis(600));
        return Ok(());
    }

    clear();
    draw_header();
    tln!("  {GREEN}✓{RESET}  Setup complete.");
    tln!();
    tln!("  Set nlsh as your default shell?");
    tln!("  {DIM}Every new terminal will open with NL routing enabled.{RESET}");
    tln!();
    tln!("  {DIM}[y] yes   [n / Enter] skip{RESET}");
    std::io::stdout().flush().ok();

    loop {
        match read_key() {
            Some(Key::Char('y')) | Some(Key::Char('Y')) => {
                tln!();
                tln!("  Your login password may be required.");
                tln!();
                std::io::stdout().flush().ok();

                // Ensure NLSH_BIN is in /etc/shells before calling chsh.
                let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
                if !shells.lines().any(|l| l.trim() == NLSH_BIN) {
                    let _ = std::process::Command::new("sh")
                        .args(["-c", &format!("echo '{NLSH_BIN}' | sudo tee -a /etc/shells")])
                        .status();
                }

                // Restore cooked terminal so chsh's password prompt renders correctly.
                let status = crate::terminal::with_cooked_mode(|| {
                    std::process::Command::new("chsh")
                        .args(["-s", NLSH_BIN])
                        .status()
                })?;

                tln!();
                if status.map(|s| s.success()).unwrap_or(false) {
                    tln!("  {GREEN}✓{RESET}  Default shell set to nlsh.");
                    tln!("  {DIM}Open a new terminal to start using it.{RESET}");
                } else {
                    tln!("  {YELLOW}Could not set default shell automatically.{RESET}");
                    tln!("  Run this command to do it manually:");
                    tln!("    {CYAN}chsh -s {NLSH_BIN}{RESET}");
                }
                tln!();
                tln!("  {DIM}[Enter] continue{RESET}");
                std::io::stdout().flush().ok();
                loop {
                    match read_key() {
                        Some(Key::Enter) | Some(Key::Char(_)) => break,
                        _ => {}
                    }
                }
                break;
            }
            Some(Key::Char('n'))
            | Some(Key::Char('N'))
            | Some(Key::Enter)
            | Some(Key::Escape) => {
                tln!();
                tln!("  {DIM}Skipped. Run {RESET}{CYAN}chsh -s {NLSH_BIN}{RESET}{DIM} anytime to change.{RESET}");
                tln!();
                tln!("  Starting nlsh...");
                std::io::stdout().flush().ok();
                std::thread::sleep(std::time::Duration::from_millis(600));
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1_073_741_824 {
        format!("{:.1} GB", n as f64 / 1_073_741_824.0)
    } else if n >= 1_048_576 {
        format!("{:.0} MB", n as f64 / 1_048_576.0)
    } else if n >= 1024 {
        format!("{:.0} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}
