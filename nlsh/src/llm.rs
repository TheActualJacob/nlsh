use anyhow::Result;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::config::{Backend, NlshConfig};

const BUILD_PATH: &str = env!("NLSH_MODEL_BUILD_PATH");

#[derive(Debug)]
pub enum LlmError {
    /// The configured backend is not available on this device / not running.
    Unavailable,
    Other(anyhow::Error),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Unavailable => write!(f, "model not available"),
            LlmError::Other(e) => write!(f, "{e}"),
        }
    }
}

pub fn shim_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.parent().unwrap_or(exe.as_path()).join("nlsh-model");
        if candidate.exists() {
            return candidate;
        }
    }
    if !BUILD_PATH.is_empty() {
        let p = PathBuf::from(BUILD_PATH);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("nlsh-model")
}

/// Check whether the Apple Intelligence shim is available (used during setup
/// before a config exists).
pub fn check_apple_shim() -> bool {
    Command::new(shim_path())
        .arg("--check")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check whether the configured backend is ready.
pub fn check_available(config: &NlshConfig) -> bool {
    match config.backend {
        Backend::Apple => check_apple_shim(),
        Backend::Ollama => {
            crate::ollama::is_running(&config.ollama_url)
                && crate::ollama::has_model(&config.ollama_url, &config.ollama_model)
        }
    }
}

/// Send `prompt` to the configured backend and return the response.
/// Displays a thinking indicator while waiting.
pub fn generate(prompt: &str, config: &NlshConfig) -> Result<String, LlmError> {
    match config.backend {
        Backend::Apple => generate_apple(prompt),
        Backend::Ollama => {
            print!("\r\x1b[2K  \x1b[36m⟳\x1b[0m thinking...");
            std::io::stdout().flush().ok();
            let result =
                crate::ollama::generate(&config.ollama_url, &config.ollama_model, prompt);
            print!("\r\x1b[2K");
            std::io::stdout().flush().ok();
            result
        }
    }
}

fn generate_apple(prompt: &str) -> Result<String, LlmError> {
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

    print!("\r\x1b[2K  \x1b[36m⟳\x1b[0m thinking...");
    std::io::stdout().flush().ok();

    let output = child
        .wait_with_output()
        .map_err(|e| LlmError::Other(e.into()))?;

    print!("\r\x1b[2K");
    std::io::stdout().flush().ok();

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return if code == 1 {
            Err(LlmError::Unavailable)
        } else if stderr.contains("guardrailViolation") {
            Err(LlmError::Other(anyhow::anyhow!(
                "Apple Intelligence declined to answer this request (safety filter)"
            )))
        } else {
            Err(LlmError::Other(anyhow::anyhow!("nlsh-model exited {code}")))
        };
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
