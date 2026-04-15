use anyhow::Result;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const BUILD_PATH: &str = env!("NLSH_MODEL_BUILD_PATH");

#[derive(Debug)]
pub enum LlmError {
    /// Apple Intelligence is not available on this device.
    Unavailable,
    Other(anyhow::Error),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Unavailable => write!(f, "Apple Intelligence not available"),
            LlmError::Other(e) => write!(f, "{e}"),
        }
    }
}

pub fn shim_path() -> PathBuf {
    // 1. Prefer sibling of current executable (release install).
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.parent().unwrap_or(exe.as_path()).join("nlsh-model");
        if candidate.exists() {
            return candidate;
        }
    }
    // 2. Fall back to build-time path (cargo run / dev).
    if !BUILD_PATH.is_empty() {
        let p = PathBuf::from(BUILD_PATH);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("nlsh-model") // last-ditch: hope it's on $PATH
}

/// Returns true if Apple Intelligence is available via the nlsh-model shim.
pub fn check_available() -> bool {
    Command::new(shim_path())
        .arg("--check")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Send `prompt` to the Foundation Models shim, wait for the full response,
/// and return the response text. Displays a thinking indicator while waiting.
pub fn generate(prompt: &str) -> Result<String, LlmError> {
    let mut child = Command::new(shim_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                LlmError::Unavailable
            } else {
                LlmError::Other(e.into())
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).ok();
    }

    // Show thinking indicator while waiting.
    print!("\r\x1b[2K  \x1b[36m⟳\x1b[0m thinking...");
    std::io::stdout().flush().ok();

    let output = child.wait_with_output().map_err(|e| LlmError::Other(e.into()))?;

    print!("\r\x1b[2K"); // clear thinking line
    std::io::stdout().flush().ok();

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        return if code == 1 {
            Err(LlmError::Unavailable)
        } else {
            Err(LlmError::Other(anyhow::anyhow!("nlsh-model exited {code}")))
        };
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
