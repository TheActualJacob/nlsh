use nix::sys::termios::{cfmakeraw, tcgetattr, tcsetattr, SetArg};
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
