use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    Apple,
    Ollama,
}

impl Backend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Backend::Apple => "apple",
            Backend::Ollama => "ollama",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NlshConfig {
    pub backend: Backend,
    pub ollama_model: String,
    pub ollama_url: String,
}

impl Default for NlshConfig {
    fn default() -> Self {
        NlshConfig {
            backend: Backend::Apple,
            ollama_model: "qwen2.5-coder:1.5b".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
        }
    }
}

impl NlshConfig {
    pub fn config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
            .join(".config")
            .join("nlsh")
            .join("config.toml")
    }

    /// Returns `Ok(None)` when no config file exists (first run).
    pub fn load() -> Result<Option<Self>> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let mut cfg = NlshConfig::default();
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim().trim_matches('"');
                match key {
                    "backend" => {
                        cfg.backend = if val == "ollama" {
                            Backend::Ollama
                        } else {
                            Backend::Apple
                        };
                    }
                    "ollama_model" => cfg.ollama_model = val.to_string(),
                    "ollama_url" => cfg.ollama_url = val.to_string(),
                    _ => {}
                }
            }
        }
        Ok(Some(cfg))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = format!(
            "# nlsh configuration\nbackend = \"{}\"\nollama_model = \"{}\"\nollama_url = \"{}\"\n",
            self.backend.as_str(),
            self.ollama_model,
            self.ollama_url,
        );
        std::fs::write(&path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_apple() {
        let cfg = NlshConfig {
            backend: Backend::Apple,
            ollama_model: "qwen2.5-coder:1.5b".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
        };
        // Serialize to string
        let content = format!(
            "backend = \"{}\"\nollama_model = \"{}\"\nollama_url = \"{}\"\n",
            cfg.backend.as_str(),
            cfg.ollama_model,
            cfg.ollama_url,
        );
        // Parse it back
        let mut out = NlshConfig::default();
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim().trim_matches('"');
                match k.trim() {
                    "backend" => out.backend = if v == "ollama" { Backend::Ollama } else { Backend::Apple },
                    "ollama_model" => out.ollama_model = v.to_string(),
                    "ollama_url" => out.ollama_url = v.to_string(),
                    _ => {}
                }
            }
        }
        assert_eq!(out.backend, Backend::Apple);
        assert_eq!(out.ollama_model, "qwen2.5-coder:1.5b");
    }

    #[test]
    fn round_trip_ollama() {
        let content = "backend = \"ollama\"\nollama_model = \"qwen2.5-coder:3b\"\nollama_url = \"http://localhost:11434\"\n";
        let mut cfg = NlshConfig::default();
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let v = v.trim().trim_matches('"');
                match k.trim() {
                    "backend" => cfg.backend = if v == "ollama" { Backend::Ollama } else { Backend::Apple },
                    "ollama_model" => cfg.ollama_model = v.to_string(),
                    "ollama_url" => cfg.ollama_url = v.to_string(),
                    _ => {}
                }
            }
        }
        assert_eq!(cfg.backend, Backend::Ollama);
        assert_eq!(cfg.ollama_model, "qwen2.5-coder:3b");
    }
}
