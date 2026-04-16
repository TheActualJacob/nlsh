mod classify;
mod config;
mod confirm;
mod intercept;
mod llm;
mod ollama;
mod parser;
mod prompt;
mod pty;
mod safety;
mod setup;
mod signals;
mod terminal;

use anyhow::Result;
use clap::Parser;
use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Parser, Debug)]
#[command(
    name = "nlsh",
    about = "Natural language shell — type plain English, get shell commands"
)]
struct Args {
    /// Show the generated command without executing it.
    #[arg(long)]
    dry_run: bool,

    /// Prefix LLM-generated commands with a space (suppressed by HIST_IGNORE_SPACE).
    #[arg(long)]
    no_hist: bool,

    /// Install nlsh as a login shell (/usr/local/bin/nlsh + /etc/shells).
    #[arg(long)]
    install: bool,

    /// Re-run model setup (choose backend / download model).
    #[arg(long)]
    setup: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.install {
        return cmd_install();
    }

    run(args)
}

fn run(args: Args) -> Result<()> {
    // ── Signal handlers ──────────────────────────────────────────────────────
    signals::install();

    // ── Load or create config ────────────────────────────────────────────────
    let existing = config::NlshConfig::load()?;
    let cfg = if args.setup || existing.is_none() {
        // First run or explicit --setup: show TUI.
        setup::run_setup()?
    } else {
        existing.unwrap()
    };

    // ── Backend availability check ───────────────────────────────────────────
    let nl_disabled = if !llm::check_available(&cfg) {
        let backend_name = match cfg.backend {
            config::Backend::Apple => "Apple Intelligence",
            config::Backend::Ollama => "Ollama",
        };
        eprintln!("[nlsh: {backend_name} unavailable — NL routing disabled]");
        true
    } else {
        false
    };

    // ── Terminal size ────────────────────────────────────────────────────────
    let (cols, rows) = terminal::get_terminal_size();

    // ── Spawn child shell ────────────────────────────────────────────────────
    let mut session = pty::spawn(cols, rows)?;

    let master_reader = session.clone_reader()?;
    let master_writer: Box<dyn std::io::Write + Send> = Box::new(session.clone_writer()?);
    let slave_fd = session.slave_fd;

    // ── Enter raw mode (RAII — restored on drop) ─────────────────────────────
    let _raw = terminal::RawMode::enter()?;

    // ── Passthrough flag (set by output thread when alternate screen active) ──
    let passthrough = Arc::new(AtomicBool::new(false));
    let passthrough_out = passthrough.clone();

    // ── Output thread: pty master → host stdout ──────────────────────────────
    std::thread::spawn(move || {
        let mut reader = master_reader;
        let mut stdout = std::io::stdout();
        let mut buf = [0u8; 4096];

        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => {
                    signals::CHILD_EXITED.store(true, Ordering::Relaxed);
                    signals::interrupt_stdin();
                    break;
                }
                Ok(n) => {
                    let chunk = &buf[..n];

                    if chunk.windows(8).any(|w| w == b"\x1b[?1049h") {
                        passthrough_out.store(true, Ordering::Relaxed);
                    }
                    if chunk.windows(8).any(|w| w == b"\x1b[?1049l") {
                        passthrough_out.store(false, Ordering::Relaxed);
                    }

                    stdout.write_all(chunk).ok();
                    stdout.flush().ok();
                }
            }
        }
    });

    // ── SIGWINCH thread: resize pty when host terminal is resized ────────────
    let master_fd_for_resize = session.master_fd;
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if signals::CHILD_EXITED.load(Ordering::Relaxed) {
            break;
        }
        if signals::SIGWINCH_RECEIVED.swap(false, Ordering::Relaxed) {
            let (cols, rows) = terminal::get_terminal_size();
            let ws = libc::winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            unsafe { libc::ioctl(master_fd_for_resize, libc::TIOCSWINSZ, &ws) };
        }
    });

    // ── Stdin intercept loop (main thread) ───────────────────────────────────
    let intercept = intercept::InterceptLoop {
        master_writer,
        passthrough,
        nl_disabled,
        dry_run: args.dry_run,
        no_hist: args.no_hist,
        config: cfg,
        slave_fd,
    };
    let _ = intercept.run();

    // ── Wait for child and exit with its status code ──────────────────────────
    let code = match session.child.wait() {
        Ok(status) => {
            if status.success() {
                0
            } else {
                1
            }
        }
        Err(_) => 1,
    };

    std::process::exit(code);
}

fn cmd_install() -> Result<()> {
    let exe = std::env::current_exe()?;
    let target = std::path::Path::new("/usr/local/bin/nlsh");

    if target.exists() {
        println!("nlsh already installed at {}", target.display());
    } else {
        match std::fs::copy(&exe, target) {
            Ok(_) => {
                println!("Copied to {}", target.display());
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                println!("Permission denied. Run:");
                println!(
                    "  sudo cp {} /usr/local/bin/nlsh && sudo chmod +x /usr/local/bin/nlsh",
                    exe.display()
                );
            }
            Err(e) => return Err(e.into()),
        }
    }

    let shim_src = llm::shim_path();
    let shim_target = std::path::Path::new("/usr/local/bin/nlsh-model");
    if shim_src.exists() {
        match std::fs::copy(&shim_src, shim_target) {
            Ok(_) => {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(shim_target, std::fs::Permissions::from_mode(0o755)).ok();
                println!("Copied nlsh-model to {}", shim_target.display());
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                println!("Permission denied for nlsh-model. Run:");
                println!(
                    "  sudo cp {} /usr/local/bin/nlsh-model && sudo chmod +x /usr/local/bin/nlsh-model",
                    shim_src.display()
                );
            }
            Err(e) => eprintln!("Warning: could not copy nlsh-model: {e}"),
        }
    } else {
        println!(
            "Warning: nlsh-model shim not found at {} — NL routing will be disabled",
            shim_src.display()
        );
    }

    let shells_path = "/etc/shells";
    let shells = std::fs::read_to_string(shells_path)?;
    let entry = "/usr/local/bin/nlsh";

    if shells.lines().any(|l| l.trim() == entry) {
        println!("Already in /etc/shells");
    } else {
        match std::fs::OpenOptions::new().append(true).open(shells_path) {
            Ok(mut f) => {
                writeln!(f, "{entry}")?;
                println!("Added to /etc/shells");
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                println!("Permission denied for /etc/shells. Run:");
                println!("  echo '{entry}' | sudo tee -a /etc/shells");
            }
            Err(e) => return Err(e.into()),
        }
    }

    println!("\nTo set nlsh as your default shell:");
    println!("  chsh -s /usr/local/bin/nlsh");
    Ok(())
}
