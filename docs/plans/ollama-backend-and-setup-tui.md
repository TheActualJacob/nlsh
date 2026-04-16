# Ollama Backend + Model Setup TUI

## Goal

Add Ollama as a second LLM backend (recommended model: `qwen2.5-coder:1.5b`), with a sleek terminal setup UI that lets users choose between Apple Intelligence and an Ollama model on first run or via `--setup`.

---

## Constraints

- macOS only (existing constraint; Ollama also supports macOS).
- No async runtime — architecture is synchronous blocking threads. All new I/O must be sync.
- No network deps today. Ollama backend adds HTTP to `localhost:11434` only (never leaves machine).
- Config must persist across sessions: `~/.config/nlsh/config.toml`.
- Apple Intelligence path must be 100% unchanged in behavior when selected.
- Setup TUI runs outside the pty session (before `pty::spawn`), so raw mode is available but no child shell is running yet.
- `ureq` 2.x is the correct sync HTTP crate — do not use `reqwest` (async, heavy).

---

## Unknowns / Risks

- **Ollama install detection**: `which ollama` or checking `~/.local/bin/ollama` / `/usr/local/bin/ollama`. If Ollama is not installed, we cannot silently install it (requires a .pkg on macOS). Must show install URL and exit setup with instructions.
- **Ollama pull streaming format**: `POST /api/pull` returns newline-delimited JSON objects with `{"status":"...","completed":N,"total":N}`. Total may not be present on early lines — handle gracefully with a spinner until total is known.
- **ureq + serde_json versions**: verify compatibility at time of implementation. As of 2026-04, `ureq = "2"` + `serde_json = "1"` are stable.
- **Config file directory creation**: `~/.config/nlsh/` may not exist — must `create_dir_all` before writing.
- **First-run detection**: config absence = first run. If config exists but `backend` is invalid, treat as first run (re-run setup).
- **Model availability check at startup**: if config says `ollama` but Ollama is not running, must fall back gracefully (print warning, disable NL routing) — same pattern as existing Apple Intelligence unavailability.
- **`intercept.rs` comment**: line 11 already says "NL routing is disabled when Ollama is unavailable" — this was forward-looking placeholder text. Plan formalizes it.

---

## Recommended Model

**`qwen2.5-coder:1.5b`** (Ollama tag: `qwen2.5-coder:1.5b`)

- Size: ~986 MB (Q4_K_M quantization)
- Context: 128K tokens (overkill for our prompt, but not a problem)
- Instruction following: top-tier for its size class
- Shell/code knowledge: purpose-built coder model, excellent at single-command output
- Speed on Apple Silicon: ~50–80 tokens/sec (M1), negligible latency vs Apple Intelligence
- Alternative if user wants higher quality: `qwen2.5-coder:3b` (~2 GB)

**Why not llama3.2:1b**: weaker instruction following, misses "respond with only a command" rules more often.
**Why not phi3.5**: 3.8B, larger download, no meaningful quality gain for 1-token shell outputs.
**Why not deepseek-coder:1.3b**: older architecture, outclassed by Qwen2.5-Coder.

---

## Steps

### 1. Add dependencies to `nlsh/Cargo.toml`

Add:
```toml
ureq = { version = "2", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### 2. Create `nlsh/src/config.rs`

```
Struct: NlshConfig { backend: Backend, ollama_model: String, ollama_url: String }
Enum: Backend { Apple, Ollama }
```

- `NlshConfig::load()` → reads `~/.config/nlsh/config.toml`, returns `Ok(None)` if absent (first run).
- `NlshConfig::save()` → writes TOML. Creates `~/.config/nlsh/` dir if absent.
- `NlshConfig::default()` → `Backend::Apple`, model `"qwen2.5-coder:1.5b"`, url `"http://localhost:11434"`.
- Parse TOML manually (3 keys, no need for `toml` crate): scan lines for `key = "value"` pattern.

### 3. Create `nlsh/src/ollama.rs`

Functions:
- `is_running(url: &str) -> bool` — `GET {url}/api/tags`, returns true on 200.
- `has_model(url: &str, model: &str) -> bool` — parse `/api/tags` JSON, check models list.
- `pull_model(url: &str, model: &str, progress_cb: impl Fn(u64, u64))` — `POST {url}/api/pull {"name": model, "stream": true}`, read newline-delimited JSON, call `progress_cb(completed, total)` each line.
- `generate(url: &str, model: &str, prompt: &str) -> Result<String, LlmError>` — `POST {url}/api/generate {"model": model, "prompt": prompt, "stream": false}`, parse `response` field.

`ureq` is sync; all calls block. No threads needed.

### 4. Modify `nlsh/src/llm.rs`

- Add `pub enum Backend { Apple, Ollama { url: String, model: String } }`.
- Change `generate(prompt: &str)` signature to `generate(prompt: &str, backend: &Backend)`.
- Add `check_available(backend: &Backend) -> bool` dispatch.
- Ollama `generate` path: calls `ollama::generate()`.
- Apple path: existing shim subprocess logic, unchanged.
- Keep `shim_path()` and Apple shim code intact.

### 5. Create `nlsh/src/setup.rs`

`pub fn run_setup() -> Result<NlshConfig>`

Terminal setup UI flow (runs in raw mode, pre-pty):

**Screen 1 — Backend selection:**
```
╭──────────────────────────────────────────╮
│           nlsh — model setup             │
╰──────────────────────────────────────────╯

  Choose an AI backend:

  ▶ 1  Apple Intelligence   on-device, no download
    2  Ollama local model   ~986 MB download

  [↑↓ / 1-2] select   [Enter] confirm   [q] quit
```
- Arrow keys + Enter or numeric keys.
- If Apple Intelligence not available (check via `llm::check_available(&Backend::Apple)`), dim option 1 and show `(unavailable on this device)`.

**Screen 2a — If Ollama selected, check Ollama install:**
- If `which ollama` fails:
  ```
  ╭──────────────────────────────────────────╮
  │           nlsh — Ollama required         │
  ╰──────────────────────────────────────────╯

    Ollama is not installed.

    Install from: https://ollama.com/download
    Then run: nlsh --setup

  [q] quit
  ```
  Exit setup, return error. User must install Ollama first.

- If Ollama is installed but not running:
  ```
    Starting Ollama...
  ```
  Attempt `Command::new("ollama").arg("serve")` in background. Wait up to 5s for `is_running()`. If still not running, show error.

**Screen 2b — Model selection (Ollama chosen, Ollama running):**
```
  Select model:

  ▶ 1  qwen2.5-coder:1.5b   ~986 MB   recommended
    2  qwen2.5-coder:3b     ~2.0 GB   higher quality
    3  Enter custom model name...

  [↑↓ / 1-3] select   [Enter] confirm   [Esc] back
```

**Screen 3 — Download (if model not already pulled):**
```
  Pulling qwen2.5-coder:1.5b...

  ████████████████░░░░░░░░   67%   661 MB / 986 MB

  [Ctrl+C] cancel
```
- If model already present locally (via `has_model()`), skip download screen entirely.
- Progress bar: 24 chars wide. `completed/total * 24` filled blocks. Rewrite same line via `\r\x1b[2K`.
- On completion:
  ```
  ✓  qwen2.5-coder:1.5b ready

  [Enter] start nlsh
  ```

All screens: 80-column friendly, ANSI colors (cyan for selection cursor, green for checkmarks, yellow for warnings, dim for unavailable items). No external TUI crate needed — pure ANSI escape codes.

Use `terminal::RawMode::enter()` at start of setup, drop before returning.

### 6. Modify `nlsh/src/main.rs`

- Add `--setup` flag to `Args`.
- Add `mod config`, `mod ollama`, `mod setup` declarations.
- In `run()`, before any other logic:
  ```
  let config = match config::NlshConfig::load()? {
      Some(c) => c,
      None => setup::run_setup()?,  // first run
  };
  ```
- If `args.setup`, call `setup::run_setup()?` regardless of config presence; overwrite saved config.
- Build `llm::Backend` from config, pass to `llm::check_available()` and throughout.
- Change `nl_disabled` check to use the config backend.
- Pass `backend` into `InterceptLoop` (new field).

### 7. Modify `nlsh/src/intercept.rs`

- Add `pub backend: llm::Backend` field to `InterceptLoop`.
- In `handle_nl()`, pass `&self.backend` to `llm::generate()`.
- Adjust error message: "NL routing disabled" message to be backend-agnostic.

### 8. Update `nlsh/src/main.rs` → `cmd_install()`

- No change needed. `config.toml` lives in `~/.config/nlsh/`, not next to the binary.

### 9. Handle `--setup` flag in release binary

- `--setup` re-runs the setup TUI and overwrites `~/.config/nlsh/config.toml`.
- After setup completes, print confirmation and exit (do not start a shell session).

---

## File Diff Summary

| File | Change |
|---|---|
| `nlsh/Cargo.toml` | Add `ureq`, `serde`, `serde_json` |
| `nlsh/src/config.rs` | **New** — config struct, load/save |
| `nlsh/src/ollama.rs` | **New** — Ollama API client |
| `nlsh/src/setup.rs` | **New** — setup TUI |
| `nlsh/src/llm.rs` | Add `Backend` enum, dispatch to ollama/apple |
| `nlsh/src/intercept.rs` | Add `backend` field, pass to `llm::generate` |
| `nlsh/src/main.rs` | `--setup` flag, config load, first-run detect |

---

## Verification

1. **Config load/save**: `cargo test` — add unit tests in `config.rs` for round-trip serialize/deserialize.
2. **Ollama detect**: with Ollama running, `ollama::is_running("http://localhost:11434")` returns `true`. With Ollama stopped, returns `false`.
3. **First run flow**: delete `~/.config/nlsh/config.toml`, run `cargo run`. Setup TUI appears.
4. **Apple selection**: choose Apple Intelligence → config saved with `backend = "apple"` → shell starts with Apple shim.
5. **Ollama selection + pull**: choose Ollama + qwen2.5-coder:1.5b → progress bar shown → model pulled → config saved → `cargo run` again skips setup, uses Ollama.
6. **Ollama generate**: type a natural language request → `⟳ thinking...` → Ollama returns command → confirm dialog shown. Latency under 2s on M-series chip.
7. **Ollama unavailable at runtime**: stop Ollama, start nlsh → `[nlsh: Ollama unavailable — NL routing disabled]` printed, shell still opens.
8. **`--setup` flag**: `nlsh --setup` from installed binary → setup TUI reruns, config overwritten.
9. **Apple fallback unchanged**: on device without Ollama, config `backend = "apple"` → identical behavior to v0.1.0.
10. **`cargo build --release`**: builds clean with no warnings.
