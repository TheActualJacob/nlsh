use std::sync::atomic::{AtomicBool, Ordering};

pub static SIGWINCH_RECEIVED: AtomicBool = AtomicBool::new(false);
pub static CHILD_EXITED: AtomicBool = AtomicBool::new(false);

extern "C" fn sigwinch_handler(_: libc::c_int) {
    SIGWINCH_RECEIVED.store(true, Ordering::Relaxed);
}

extern "C" fn sighup_handler(_: libc::c_int) {
    CHILD_EXITED.store(true, Ordering::Relaxed);
}

pub fn install() {
    unsafe {
        libc::signal(libc::SIGWINCH, sigwinch_handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, sighup_handler as *const () as libc::sighandler_t);
        // Ignore SIGPIPE — standard practice for terminal tools.
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

/// Send SIGHUP to our own process to interrupt a blocking stdin read.
pub fn interrupt_stdin() {
    unsafe {
        libc::kill(libc::getpid(), libc::SIGHUP);
    }
}
