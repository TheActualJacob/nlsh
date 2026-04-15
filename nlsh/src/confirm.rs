use anyhow::Result;
use std::io::{Read, Write};

pub enum ConfirmResult {
    Execute(String),
    Cancel,
}

/// Display a confirmation prompt for `cmd` and wait for a single keypress.
///
/// Terminal must already be in raw mode.
/// `destructive` = true prepends a red warning prefix.
pub fn prompt(cmd: &str, destructive: bool) -> Result<ConfirmResult> {
    let mut stdout = std::io::stdout();

    // Clear the current line and render the prompt.
    if destructive {
        print!("\r\x1b[2K  \x1b[1;31m⚠ DESTRUCTIVE:\x1b[0m {cmd}");
    } else {
        print!("\r\x1b[2K  \x1b[1;32m❯\x1b[0m {cmd}");
    }
    print!("  \x1b[2m[enter=run  e=edit  n=cancel]\x1b[0m");
    stdout.flush()?;

    // Read one keypress (terminal is in raw mode).
    let mut buf = [0u8; 1];
    std::io::stdin().read_exact(&mut buf)?;

    match buf[0] {
        b'\r' | b'\n' => {
            println!();
            Ok(ConfirmResult::Execute(cmd.to_string()))
        }
        b'e' | b'E' => {
            println!();
            let edited = open_editor(cmd)?;
            if edited.is_empty() {
                println!("  \x1b[2m[empty command — cancelled]\x1b[0m");
                return Ok(ConfirmResult::Cancel);
            }
            // Re-confirm with edited command.
            prompt(&edited, crate::safety::is_destructive(&edited))
        }
        _ => {
            println!("\r\x1b[2K  \x1b[2m[cancelled]\x1b[0m");
            Ok(ConfirmResult::Cancel)
        }
    }
}

fn open_editor(initial: &str) -> Result<String> {
    use std::io::Write as _;

    let mut tmp = tempfile::NamedTempFile::new()?;
    write!(tmp, "{initial}")?;
    tmp.flush()?;

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());

    // Editor manages its own terminal mode; our RawMode drop will restore
    // settings cleanly after the editor exits.
    std::process::Command::new(&editor)
        .arg(tmp.path())
        .status()?;

    let content = std::fs::read_to_string(tmp.path())?;
    Ok(content.trim().to_string())
}
