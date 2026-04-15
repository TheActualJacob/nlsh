use anyhow::Result;
use std::io::Write;
use std::sync::atomic::Ordering;

pub struct InterceptLoop {
    /// Write end of the pty master — sends input to the child shell.
    pub master_writer: Box<dyn Write + Send>,
    /// Shared flag: true while an interactive program (vim, ssh, etc.) is
    /// running and we should pass through all input without buffering.
    pub passthrough: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// NL routing is disabled when Ollama is unavailable at startup.
    pub nl_disabled: bool,
    /// --dry-run: show the generated command but do not execute it.
    pub dry_run: bool,
    /// --no-hist: prefix commands with a space so HIST_IGNORE_SPACE suppresses
    /// them in the child shell's history.
    pub no_hist: bool,
}

impl InterceptLoop {
    pub fn run(mut self) -> Result<()> {
        let mut stdout = std::io::stdout();
        let mut line_buf: Vec<u8> = Vec::with_capacity(256);

        // Escape-sequence accumulation state.
        let mut esc_buf: Vec<u8> = Vec::with_capacity(8);
        let mut in_esc = false;

        // Bracketed-paste state.
        let mut in_paste = false;

        loop {
            // Check if child has exited (set by output thread via SIGHUP).
            if crate::signals::CHILD_EXITED.load(Ordering::Relaxed) {
                break;
            }

            let byte = match read_byte() {
                Some(b) => b,
                None => {
                    // EINTR or EOF.
                    if crate::signals::CHILD_EXITED.load(Ordering::Relaxed) {
                        break;
                    }
                    continue;
                }
            };

            // ── Passthrough mode (interactive program running) ───────────────
            if self.passthrough.load(Ordering::Relaxed) || in_paste {
                self.master_writer.write_all(&[byte])?;
                self.master_writer.flush().ok();
                continue;
            }

            // ── Escape sequence handling ─────────────────────────────────────
            if in_esc {
                esc_buf.push(byte);

                // Bracketed-paste start: ESC [ ? 2 0 0 4 h
                if esc_buf == b"\x1b[?2004h" {
                    in_paste = true;
                    in_esc = false;
                    // Forward the sequence; child shell will handle paste mode.
                    self.master_writer.write_all(&esc_buf)?;
                    esc_buf.clear();
                    continue;
                }
                // Bracketed-paste end: ESC [ ? 2 0 0 4 l
                if esc_buf == b"\x1b[?2004l" {
                    in_paste = false;
                    in_esc = false;
                    self.master_writer.write_all(&esc_buf)?;
                    esc_buf.clear();
                    continue;
                }

                // Escape sequence terminated by a letter or '~'.
                if esc_buf.len() > 1 && (byte.is_ascii_alphabetic() || byte == b'~') {
                    // Flush any buffered line text then the escape sequence —
                    // the user is navigating history / moving the cursor.
                    self.master_writer.write_all(&line_buf)?;
                    self.master_writer.write_all(&esc_buf)?;
                    self.master_writer.flush().ok();
                    line_buf.clear();
                    in_esc = false;
                    esc_buf.clear();
                }
                // Keep accumulating if sequence not yet terminated.
                continue;
            }

            if byte == 0x1b {
                in_esc = true;
                esc_buf.clear();
                esc_buf.push(byte);
                continue;
            }

            // ── Normal byte handling ─────────────────────────────────────────
            match byte {
                // Tab → forward buffer + Tab immediately; let ZLE handle completion.
                0x09 => {
                    self.master_writer.write_all(&line_buf)?;
                    self.master_writer.write_all(b"\t")?;
                    self.master_writer.flush().ok();
                    line_buf.clear();
                }

                // Ctrl+C → discard buffer, forward signal byte.
                0x03 => {
                    if !line_buf.is_empty() {
                        // Erase the echoed text visually.
                        erase_line(&mut stdout, line_buf.len());
                        line_buf.clear();
                    }
                    self.master_writer.write_all(&[byte])?;
                    self.master_writer.flush().ok();
                }

                // Ctrl+D → forward only when buffer is empty (shell EOF / logout).
                0x04 => {
                    if line_buf.is_empty() {
                        self.master_writer.write_all(&[byte])?;
                        self.master_writer.flush().ok();
                    }
                }

                // Backspace / DEL.
                0x7f | 0x08 => {
                    if !line_buf.is_empty() {
                        line_buf.pop();
                        stdout.write_all(b"\x08 \x08")?;
                        stdout.flush()?;
                    }
                }

                // Enter (CR or LF).
                0x0d | 0x0a => {
                    let line = String::from_utf8_lossy(&line_buf).into_owned();
                    line_buf.clear();
                    // Move to next line on the host terminal.
                    stdout.write_all(b"\r\n")?;
                    stdout.flush()?;
                    if let Err(e) = self.handle_line(&line) {
                        eprintln!("\r[nlsh error: {e}]");
                    }
                }

                // Printable ASCII.
                0x20..=0x7e => {
                    line_buf.push(byte);
                    stdout.write_all(&[byte])?;
                    stdout.flush()?;
                }

                // Other control bytes — forward as-is.
                _ => {
                    self.master_writer.write_all(&[byte])?;
                    self.master_writer.flush().ok();
                }
            }
        }
        Ok(())
    }

    fn handle_line(&mut self, line: &str) -> Result<()> {
        if self.nl_disabled {
            return self.send_to_shell(line);
        }

        use crate::classify::{classify, LineKind};
        match classify(line) {
            LineKind::Shell => self.send_to_shell(line),
            LineKind::NaturalLanguage => self.handle_nl(line),
        }
    }

    fn send_to_shell(&mut self, line: &str) -> Result<()> {
        if self.no_hist && !line.trim().is_empty() {
            // Prefix with space; requires HIST_IGNORE_SPACE in user's .zshrc.
            self.master_writer.write_all(b" ")?;
        }
        self.master_writer.write_all(line.as_bytes())?;
        self.master_writer.write_all(b"\n")?;
        self.master_writer.flush().ok();
        Ok(())
    }

    fn handle_nl(&mut self, line: &str) -> Result<()> {
        use crate::{confirm, llm, parser, prompt as prompt_mod, safety};

        let ctx = prompt_mod::ShellContext::current();
        let full_prompt = prompt_mod::build_prompt(line, &ctx);

        match llm::generate(&full_prompt) {
            Ok(raw) => match parser::clean_llm_output(&raw) {
                Some(cmd) => {
                    if self.dry_run {
                        println!("\r\x1b[2K  \x1b[2m[dry-run]\x1b[0m {cmd}");
                        return Ok(());
                    }
                    let destructive = safety::is_destructive(&cmd);
                    match confirm::prompt(&cmd, destructive)? {
                        confirm::ConfirmResult::Execute(cmd) => self.send_to_shell(&cmd),
                        confirm::ConfirmResult::Cancel => Ok(()),
                    }
                }
                None => {
                    println!("\r\x1b[2K  \x1b[33m[nlsh: could not translate — try rephrasing]\x1b[0m");
                    Ok(())
                }
            },
            Err(llm::LlmError::Unavailable) => {
                eprintln!(
                    "\r\x1b[2K  \x1b[33m[nlsh: ollama not running — forwarding as shell command]\x1b[0m"
                );
                self.send_to_shell(line)
            }
            Err(e) => {
                eprintln!("\r\x1b[2K  \x1b[31m[nlsh: llm error: {e}]\x1b[0m");
                Ok(())
            }
        }
    }
}

/// Read a single raw byte from stdin (fd 0).
/// Returns None on EINTR or transient error; caller should retry.
/// Returns None on EOF/fatal error after setting CHILD_EXITED.
fn read_byte() -> Option<u8> {
    let mut buf = [0u8; 1];
    loop {
        let n = unsafe { libc::read(0, buf.as_mut_ptr() as *mut libc::c_void, 1) };
        if n == 1 {
            return Some(buf[0]);
        }
        if n == 0 {
            // EOF on stdin.
            return None;
        }
        // n == -1 → error
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            return None; // EINTR — let caller check flags and retry
        }
        return None; // fatal read error
    }
}

fn erase_line(stdout: &mut std::io::Stdout, char_count: usize) {
    for _ in 0..char_count {
        let _ = stdout.write_all(b"\x08 \x08");
    }
    let _ = stdout.flush();
}
