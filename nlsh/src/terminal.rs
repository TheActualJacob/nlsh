use nix::sys::termios::{cfmakeraw, tcgetattr, tcsetattr, LocalFlags, OutputFlags, InputFlags, SetArg};
use std::os::unix::io::BorrowedFd;

pub struct RawMode {
    original: nix::sys::termios::Termios,
}

impl RawMode {
    pub fn enter() -> anyhow::Result<Self> {
        // Safety: fd 0 (stdin) is always valid while the process runs.
        let fd = unsafe { BorrowedFd::borrow_raw(0) };
        let original = tcgetattr(fd)?;
        let mut raw = original.clone();
        cfmakeraw(&mut raw);
        tcsetattr(fd, SetArg::TCSANOW, &raw)?;
        Ok(RawMode { original })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let fd = unsafe { BorrowedFd::borrow_raw(0) };
        let _ = tcsetattr(fd, SetArg::TCSANOW, &self.original);
    }
}

/// Temporarily restore a usable cooked terminal, run `f`, then re-enter raw mode.
/// Used when spawning interactive subprocesses (e.g. `chsh`) from within raw mode.
pub fn with_cooked_mode<F: FnOnce() -> T, T>(f: F) -> anyhow::Result<T> {
    let fd = unsafe { BorrowedFd::borrow_raw(0) };
    let raw_settings = tcgetattr(fd)?;
    let mut cooked = raw_settings.clone();
    // Re-enable the flags cfmakeraw disabled.
    cooked.local_flags |= LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::ISIG | LocalFlags::IEXTEN;
    cooked.output_flags |= OutputFlags::OPOST;
    cooked.input_flags |= InputFlags::ICRNL;
    tcsetattr(fd, SetArg::TCSANOW, &cooked)?;
    let result = f();
    tcsetattr(fd, SetArg::TCSANOW, &raw_settings)?;
    Ok(result)
}

/// Returns (cols, rows) of the host terminal.
pub fn get_terminal_size() -> (u16, u16) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0
            && ws.ws_col > 0
            && ws.ws_row > 0
        {
            (ws.ws_col, ws.ws_row)
        } else {
            (80, 24)
        }
    }
}
