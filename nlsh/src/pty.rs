use anyhow::Result;
use nix::{
    fcntl::{fcntl, FcntlArg, FdFlag},
    pty::{openpty, Winsize},
    sys::termios::{tcgetattr, LocalFlags},
    unistd::dup,
};
use std::{
    fs::File,
    os::{
        fd::IntoRawFd,
        unix::io::{BorrowedFd, FromRawFd, OwnedFd, RawFd},
    },
    process::{Child, Stdio},
};

pub struct PtySession {
    /// Raw fd for the PTY master. Used for reading, writing, and resizing.
    pub master_fd: RawFd,
    /// Raw fd for the PTY slave. Kept open in the parent solely to check
    /// whether ECHO is currently enabled (e.g. disabled during sudo password prompts).
    pub slave_fd: RawFd,
    pub child: Child,
}

impl PtySession {
    /// Returns a `File` that reads from the PTY master (dup'd fd, owned by caller).
    pub fn clone_reader(&self) -> Result<File> {
        Ok(unsafe { File::from_raw_fd(dup(self.master_fd)?) })
    }

    /// Returns a `File` that writes to the PTY master (dup'd fd, owned by caller).
    pub fn clone_writer(&self) -> Result<File> {
        Ok(unsafe { File::from_raw_fd(dup(self.master_fd)?) })
    }

    /// Resize the PTY to the given dimensions.
    pub fn resize(&self, cols: u16, rows: u16) {
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            libc::ioctl(self.master_fd, libc::TIOCSWINSZ, &ws);
        }
    }

    /// Returns `false` when the slave PTY has ECHO disabled — i.e. the child
    /// process is reading a password. The intercept loop uses this to bypass
    /// line-buffering and forward raw bytes directly.
    pub fn echo_on(&self) -> bool {
        let fd = unsafe { BorrowedFd::borrow_raw(self.slave_fd) };
        tcgetattr(fd)
            .map(|t| t.local_flags.contains(LocalFlags::ECHO))
            .unwrap_or(true)
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.master_fd);
            libc::close(self.slave_fd);
        }
    }
}

pub fn spawn(cols: u16, rows: u16) -> Result<PtySession> {
    use std::os::unix::process::CommandExt;

    let pair = openpty(
        Some(&Winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )?;

    let master_fd: RawFd = OwnedFd::into_raw_fd(pair.master);
    let slave_fd: RawFd = OwnedFd::into_raw_fd(pair.slave);

    // Set O_CLOEXEC on the parent's slave fd so it's closed in the child after
    // exec (the child gets its own copies via dup2 for stdio).
    fcntl(slave_fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;

    // Dup slave for each of the child's stdio streams.
    // `Stdio::from_raw_fd` takes ownership; Command::spawn() dup2's each to
    // fd 0/1/2 in the child, then closes the originals.
    let slave_stdin = unsafe { Stdio::from_raw_fd(dup(slave_fd)?) };
    let slave_stdout = unsafe { Stdio::from_raw_fd(dup(slave_fd)?) };
    let slave_stderr = unsafe { Stdio::from_raw_fd(dup(slave_fd)?) };

    let child = unsafe {
        let mut cmd = std::process::Command::new("zsh");
        for var in &["TERM", "HOME", "PATH", "USER", "LANG", "LOGNAME", "SHELL"] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }
        cmd.stdin(slave_stdin)
            .stdout(slave_stdout)
            .stderr(slave_stderr)
            .pre_exec(move || {
                // Create a new session so the child has no controlling terminal.
                libc::setsid();
                // Make the slave PTY the controlling terminal for this session.
                libc::ioctl(slave_fd, libc::TIOCSCTTY.into(), 0i32);
                Ok(())
            })
            .spawn()?
    };

    Ok(PtySession {
        master_fd,
        slave_fd,
        child,
    })
}
