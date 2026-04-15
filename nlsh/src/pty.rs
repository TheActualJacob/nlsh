use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

pub struct PtySession {
    pub master: Box<dyn portable_pty::MasterPty + Send>,
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
}

pub fn spawn(cols: u16, rows: u16) -> Result<PtySession> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new("zsh");
    // Propagate essential env vars to the child shell.
    for var in &["TERM", "HOME", "PATH", "USER", "LANG", "LOGNAME", "SHELL"] {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, val);
        }
    }

    let child = pair.slave.spawn_command(cmd)?;
    Ok(PtySession {
        master: pair.master,
        child,
    })
}
