use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

const OLLAMA_BASE: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "qwen2.5-coder:7b";

#[derive(Debug)]
pub enum LlmError {
    /// Ollama is not reachable (not installed / not running).
    Unavailable,
    Other(anyhow::Error),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Unavailable => write!(f, "ollama not reachable"),
            LlmError::Other(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    options: GenerateOptions,
}

#[derive(Serialize)]
struct GenerateOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Deserialize)]
struct GenerateChunk {
    response: String,
    done: bool,
}

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(2))
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}

/// Returns true if Ollama is reachable at the default address.
pub fn check_available() -> bool {
    let Ok(c) = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()
    else {
        return false;
    };
    c.get(format!("{OLLAMA_BASE}/api/tags"))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Send `prompt` to Ollama, stream tokens to stdout as they arrive,
/// and return the full response text.
///
/// Prints a leading `❯ ` prompt indicator before tokens start streaming.
/// The caller is responsible for clearing/overwriting this line afterwards.
pub fn generate(prompt: &str) -> Result<String, LlmError> {
    let c = client().map_err(|e| LlmError::Other(e))?;

    let body = GenerateRequest {
        model: DEFAULT_MODEL,
        prompt,
        stream: true,
        options: GenerateOptions {
            temperature: 0.1,
            num_predict: 200,
        },
    };

    let response = c
        .post(format!("{OLLAMA_BASE}/api/generate"))
        .json(&body)
        .send()
        .map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                LlmError::Unavailable
            } else {
                LlmError::Other(e.into())
            }
        })?;

    if !response.status().is_success() {
        return Err(LlmError::Other(anyhow::anyhow!(
            "ollama returned {}",
            response.status()
        )));
    }

    // Show streaming indicator.
    print!("\r\x1b[2K  \x1b[36m⟳\x1b[0m ");
    std::io::stdout().flush().ok();

    let mut full = String::new();
    let reader = std::io::BufReader::new(response);

    for line in reader.lines() {
        let line = line.map_err(|e| LlmError::Other(e.into()))?;
        if line.is_empty() {
            continue;
        }
        let chunk: GenerateChunk =
            serde_json::from_str(&line).map_err(|e| LlmError::Other(e.into()))?;

        if !chunk.response.is_empty() {
            print!("{}", chunk.response);
            std::io::stdout().flush().ok();
            full.push_str(&chunk.response);
        }

        if chunk.done {
            break;
        }
    }

    println!();
    Ok(full)
}
