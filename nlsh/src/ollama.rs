use anyhow::Result;
use std::io::BufRead;

use crate::llm::LlmError;

/// Returns true if Ollama is reachable at `url`.
pub fn is_running(url: &str) -> bool {
    ureq::get(&format!("{url}/api/tags"))
        .call()
        .map(|r| r.status() == 200)
        .unwrap_or(false)
}

/// Returns true if `model` is already pulled locally.
pub fn has_model(url: &str, model: &str) -> bool {
    let Ok(resp) = ureq::get(&format!("{url}/api/tags")).call() else {
        return false;
    };
    let Ok(body) = resp.into_string() else {
        return false;
    };
    // The tags JSON has entries like {"name":"qwen2.5-coder:1.5b",...}
    body.contains(&format!("\"{}\"", model))
}

/// Pull `model` via the Ollama streaming pull API.
/// `progress_cb` is called with `(completed_bytes, total_bytes)` for each
/// progress line that has a known total.
pub fn pull_model(
    url: &str,
    model: &str,
    mut progress_cb: impl FnMut(u64, u64),
) -> Result<()> {
    let resp = ureq::post(&format!("{url}/api/pull"))
        .set("Content-Type", "application/json")
        .send_string(&format!("{{\"name\":\"{model}\",\"stream\":true}}"))?;

    for line in std::io::BufReader::new(resp.into_reader()).lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let completed = extract_num(&line, "completed").unwrap_or(0);
        let total = extract_num(&line, "total").unwrap_or(0);
        if total > 0 {
            progress_cb(completed, total);
        }
    }
    Ok(())
}

/// Send `prompt` to a running Ollama instance and return the full response text.
pub fn generate(url: &str, model: &str, prompt: &str) -> Result<String, LlmError> {
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false
    })
    .to_string();

    let resp = ureq::post(&format!("{url}/api/generate"))
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map_err(|e| LlmError::Other(anyhow::anyhow!("Ollama request failed: {e}")))?;

    let body_str = resp
        .into_string()
        .map_err(|e| LlmError::Other(e.into()))?;

    let v: serde_json::Value =
        serde_json::from_str(&body_str).map_err(|e| LlmError::Other(e.into()))?;

    v["response"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| LlmError::Other(anyhow::anyhow!("no response field in Ollama output")))
}

/// Extract a u64 from a JSON-like string by key name.
/// e.g. `extract_num(r#"{"completed":123,"total":456}"#, "completed")` → Some(123)
fn extract_num(json: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{}\":", key);
    let pos = json.find(&pattern)?;
    let after = json[pos + pattern.len()..].trim_start();
    let end = after
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after.len());
    after[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_num_basic() {
        assert_eq!(extract_num(r#"{"completed":542,"total":986}"#, "completed"), Some(542));
        assert_eq!(extract_num(r#"{"completed":542,"total":986}"#, "total"), Some(986));
        assert_eq!(extract_num(r#"{"status":"pulling"}"#, "completed"), None);
    }

    #[test]
    fn extract_num_spaces() {
        assert_eq!(extract_num(r#"{"completed": 100, "total": 200}"#, "total"), Some(200));
    }
}
