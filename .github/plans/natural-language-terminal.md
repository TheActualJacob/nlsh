# Natural Language Terminal — Implementation Plan

## Goal

Build a Rust binary that wraps zsh via a pty, intercepts user input, routes natural-language lines to a local Ollama LLM, and executes confirmed shell commands — with no cloud dependency and no shell plugin required.

---

## Constraints

- macOS only for v1 (pty behavior, `TIOCSCTTY`, `type -a` builtins are POSIX but tested only on macOS)
- Rust stable toolchain (1.77+)
- Ollama must be locally installed by the user; binary does not install it
- Requires the binary path to be added to `/etc/shells` for use as a login shell
- Ollama OpenAI-compat API is marked "experimental" upstream — pin to `/api/chat` (native) not `/v1/chat/completions` to avoid instability risk
- Phase 5 (Apple Foundation Models) deferred until Phases 1–4 ship; `afm-cli` is a third-party tool and is NOT a target for v1

---

## Unknowns / Risks

- **Line intercept vs. ZLE**: ZLE runs in raw mode; intercepting at the pty master requires the wrapper to implement its own echo/backspace handling. Decision required before Phase 2: (A) put child pty in canonical mode — lose ZLE/readline in child; (B) wrapper echoes keystrokes manually, handles backspace and common escape sequences — preserves ZLE feel but requires a mini line editor. **Recommend B.** Document the decision.
- **Bracketed paste**: terminals send `\e[?2004h`; pasted multi-line content will trigger multiple routing decisions. Unknown: desired behavior — likely should detect paste escape sequences and forward the entire paste block directly.
- **Tab completion**: completion requires the shell to receive keystrokes. With wrapper buffering, shell gets no input during intercept. Decision: forward Tab immediately (bypass intercept layer), let shell handle completion.
- **Ollama not running**: HTTP connection to `localhost:11434` will time out (default ~30s). Need explicit timeout + clear error message.
- **Model output format**: 7B models regularly return markdown fences, preamble, or multi-option responses. Parser required.
- **History writeback**: child zsh maintains its own `HISTFILE`; commands injected via pty appear in child history normally. Persistent `~/.zsh_history` writeback after session end may conflict with other concurrent zsh sessions (HIST_SHARE). Scope to v1: let child zsh manage its own history; do not do explicit `~/.zsh_history` manipulation.
- **Confirmation UI placement**: LLM result must be displayed without corrupting child shell's prompt. Requires saving/restoring cursor position or using an alternate display line.

---

## Steps

### Phase 1 — Transparent pty Wrapper

**Goal:** User runs the binary; sees and interacts with a normal zsh session. No visible difference from opening Terminal.

**1.1** Initialize Rust binary crate.
```
cargo new nlsh --bin
```
Add dependencies to `Cargo.toml`:
```toml
portable-pty = "0.8"        # pty master/slave + SIGWINCH handling
tokio = { version = "1", features = ["full"] }
nix = { version = "0.29", features = ["signal", "term"] }
```

**1.2** Allocate pty pair and spawn child zsh.
- File: `src/pty.rs`
- Use `portable_pty::native_pty_system()` → `openpty(PtySize { rows, cols, .. })`
- Spawn `CommandBuilder::new("zsh")` on slave end with `controlling_tty = true` (required for SIGWINCH delivery)
- Pass current `$TERM`, `$HOME`, `$PATH`, `$USER` env vars to child

**1.3** Implement transparent I/O loop.
- File: `src/main.rs`
- Two async tasks:
  - **stdin→slave**: read bytes from host stdin, write to pty master writer
  - **master→stdout**: read bytes from pty master reader, write to host stdout
- Put host terminal in raw mode (`nix::sys::termios::cfmakeraw`) on entry; restore on exit (RAII guard)

**1.4** Forward terminal resize (SIGWINCH).
- File: `src/signals.rs`
- Install SIGWINCH handler via `tokio::signal::unix`
- On signal: read current terminal size via `TIOCGWINSZ` ioctl on host stdout fd; call `master.resize(PtySize { ... })`
- `portable-pty` delivers resize to child via `TIOCSWINSZ` + SIGWINCH to child's process group

**1.5** Handle SIGHUP / child exit.
- Monitor child process; when it exits, restore terminal and exit wrapper with same code
- Install SIGHUP handler: forward to child process group

**Verification — Phase 1:**
- `cargo run` opens a zsh session
- `vim`, `python3`, `ssh localhost` all work interactively
- Resizing the terminal window updates `$COLUMNS`/`$LINES` inside the session
- `echo $SHELL` inside the session reflects the child zsh path
- `exit` cleanly returns to the calling shell

---

### Phase 2 — Line Intercept Layer

**Goal:** Classify each submitted line before forwarding to child shell.

**2.1** Switch stdin→slave task from transparent pass-through to a buffered line editor.
- File: `src/intercept.rs`
- Read raw bytes from host stdin one byte at a time
- Maintain a `Vec<u8>` line buffer
- Echo printable bytes back to host stdout (so user sees what they're typing)
- Handle:
  - `0x7f` / `0x08` (backspace/DEL): pop last byte from buffer, emit `\x08 \x08` to terminal to erase
  - `\t` (Tab, `0x09`): flush buffer to slave immediately without classification; let ZLE handle completion; resume buffering after
  - `\r` / `\n` (Enter): take the buffer as a complete line; run classifier; clear buffer
  - `ESC` sequences (`\x1b[...`): buffer the full escape sequence; detect arrow keys; if arrow key, flush entire buffer + escape sequence to slave (user navigating history — let ZLE run)
  - `\x03` (Ctrl+C): clear buffer; forward to slave
  - `\x04` (Ctrl+D on empty buffer): forward to slave

**2.2** Implement classifier.
- File: `src/classify.rs`
- Function signature: `fn classify(line: &str) -> LineKind` where `LineKind = { Shell, NaturalLanguage }`
- Algorithm:
  1. Trim leading whitespace
  2. Extract first token (split on whitespace)
  3. Run `type -a <token>` in a short-lived `zsh -c` subprocess (not the child shell)
  4. If exit 0 → `Shell`
  5. If exit nonzero → `NaturalLanguage`
- Edge cases:
  - Empty line → `Shell` (forward \n to child)
  - Line starts with `!` → `Shell` (history expansion)
  - Line starts with `#` → `Shell` (comment)
  - Line contains `=` as first non-whitespace → `Shell` (variable assignment)
  - First token contains `/` → `Shell` (explicit path)

**2.3** Wire classifier into intercept loop.
- `Shell` lines: write buffered line + `\n` to pty master writer
- `NaturalLanguage` lines: pass to Phase 3 LLM handler (stub: print `[NL detected: "<line>"]` and drop for now)

**Verification — Phase 2:**
- `ls -la` → executes normally
- `git status` → executes normally
- `show me disk usage` → prints `[NL detected: "show me disk usage"]`
- `find me the largest file` → `find` is in PATH → executes `find me the largest file` as a shell command (and fails; this is correct behavior — the input IS valid shell)
- Arrow keys navigate zsh history without corruption
- Tab completion works

---

### Phase 3 — LLM Integration

**Goal:** Natural-language lines are translated to shell commands; user confirms before execution.

**3.1** Add Ollama HTTP client.
- File: `src/llm.rs`
- Dependency: `reqwest = { version = "0.12", features = ["json", "stream"] }`; `serde_json`
- Use Ollama's native `/api/generate` endpoint (not `/v1/chat/completions` — the OpenAI-compat layer is marked experimental)
- Endpoint: `POST http://localhost:11434/api/generate`
- Request body:
  ```json
  {
    "model": "qwen2.5-coder:7b",
    "prompt": "<system_prompt>\n\nUser request: <line>",
    "stream": true,
    "options": { "temperature": 0.1, "num_predict": 200 }
  }
  ```
- Set `reqwest` connect timeout: 2s; read timeout: 30s
- On `ConnectionRefused` or timeout: return `Err(LlmError::Unavailable)`; print `[nlsh: ollama not running — forwarding as shell command]`; fall back to Shell routing

**3.2** Build system prompt.
- File: `src/prompt.rs`
- Function: `fn build_prompt(request: &str, context: &ShellContext) -> String`
- `ShellContext`: `{ cwd: PathBuf, user: String, shell: String }`
- Populate at startup; refresh `cwd` on each request via `std::env::current_dir()` (or read `/proc/self/cwd` — but better: send a `pwd` to the child shell and capture output — TBD, use `std::env::current_dir()` for v1)
- System prompt template:
  ```
  You are a shell command translator. The user is in a zsh session.
  Current directory: {cwd}
  User: {user}

  Rules:
  - Respond with ONLY a single shell command. No explanation. No markdown. No alternatives.
  - If the request is ambiguous, prefer the safest interpretation.
  - If the request cannot be translated to a single command, output: CANNOT_TRANSLATE
  ```

**3.3** Implement LLM output parser/cleaner.
- File: `src/parser.rs`
- Function: `fn clean_llm_output(raw: &str) -> Option<String>`
- Steps (applied in order):
  1. Strip outer markdown fences: if starts with ` ``` ` (with or without lang tag), extract content between fences
  2. Strip leading lines that don't look like a command (no `$`, no `/`, not a known binary prefix) — heuristic: if a line starts with a capital letter and has no pipe/redirect, it's explanation; drop it
  3. Trim whitespace
  4. If result is empty or equals `CANNOT_TRANSLATE` → return `None`
  5. Return `Some(cleaned_command)`

**3.4** Implement confirmation UI.
- File: `src/confirm.rs`
- After receiving and cleaning LLM output:
  1. Print a divider line to stderr (so it doesn't get captured)
  2. Print: `  ❯ <command>  [enter=run  e=edit  n=cancel]`
  3. Put terminal in raw mode; read a single keypress:
     - `\r`/`\n` → execute command
     - `e` → open `$EDITOR` (or `vi`) with command in a tempfile; on editor exit, read back modified command; re-confirm
     - `n`/`\x03`/`\x1b` → cancel; print `[cancelled]`; return to prompt
  4. On execute: write `<command>\n` to pty master writer (child shell executes it and adds to its own history)
  5. Print empty line to restore prompt aesthetics

**3.5** Stream LLM response tokens to terminal during generation.
- While waiting for `/api/generate` stream: print tokens as they arrive (shows the command being built character by character)
- On completion: move cursor back to start of that line, overwrite with clean confirmation prompt

**Verification — Phase 3:**
- `show me disk usage` → streams `df -h`, presents confirmation prompt
- Ctrl+C during streaming → cancels request, returns to prompt
- LLM returns markdown-fenced output → parser strips fences, clean command shown
- LLM returns explanation text → parser drops it, command shown
- Ollama not running → error message, input treated as shell command
- `e` at confirmation → opens editor; edited command executes correctly

---

### Phase 4 — UX Polish

**4.1** Shell history integration.
- Commands executed via LLM path are written to child zsh via the pty writer — they appear in child's `fc` / `history` output automatically
- No additional work needed for in-session history
- `--no-hist` flag: if set, prefix command with `' '` (space) before sending to child; zsh's `HIST_IGNORE_SPACE` will suppress it (document this requires `setopt HIST_IGNORE_SPACE` in user's `.zshrc`)

**4.2** `--dry-run` flag.
- Parse CLI args with `clap`
- If `--dry-run`: on NL input, run LLM, print cleaned command, do NOT present confirmation, do NOT execute

**4.3** Destructive command warning.
- File: `src/safety.rs`
- Before presenting confirmation, scan the command string for patterns:
  ```
  rm -rf, dd if=, mkfs, :(){ :|:& };:, chmod -R 777, > /dev/sda
  ```
  (static list, not LLM-based)
- If matched: prepend `⚠ DESTRUCTIVE:` in red to the confirmation prompt

**4.4** Startup Ollama check.
- On binary start (before opening pty): send a `GET http://localhost:11434/api/tags` with 1s timeout
- If fails: print warning `[nlsh: ollama unreachable — NL routing disabled]`; continue in shell-only mode
- NL-classified lines in shell-only mode: forward directly to child shell (user sees the error from the shell)

**4.5** Installation helper.
- `nlsh --install`: 
  1. Copies binary to `/usr/local/bin/nlsh` (or prints instructions if no write permission)
  2. Appends `/usr/local/bin/nlsh` to `/etc/shells` if not present (requires sudo; print command for user to run if not root)
  3. Prints `chsh -s /usr/local/bin/nlsh` for user to run manually

**Verification — Phase 4:**
- `nlsh --dry-run` shows commands without executing
- `rm -rf` command generated by LLM shows destructive warning
- Ollama not running at startup: warning shown, shell session continues normally
- `nlsh --install` appends to `/etc/shells` correctly

---

### Phase 5 — Deferred

Apple Foundation Models integration. Prerequisites:
- Phases 1–4 shipped and stable
- Decision made: Swift shim binary (recommended) vs. subprocess call to `afm-cli` (third-party, fragile)
- If Swift shim: small Swift executable calling `FoundationModels` framework; Rust calls it via `std::process::Command`; shim accepts prompt on stdin, returns command on stdout
- Model quality evaluation: on-device 3B model vs. Qwen2.5-Coder:7B on a fixed eval set of 50 NL→command pairs

---

## Crate / File Structure

```
nlsh/
  Cargo.toml
  src/
    main.rs          # CLI args, startup checks, pty lifecycle
    pty.rs           # pty allocation, child spawn, resize forwarding
    intercept.rs     # line buffering, echo, escape sequence handling
    classify.rs      # first-token classifier (type -a)
    llm.rs           # Ollama HTTP client, streaming
    prompt.rs        # system prompt builder, ShellContext
    parser.rs        # LLM output cleaner
    confirm.rs       # confirmation UI, editor integration
    safety.rs        # destructive command pattern matching
    signals.rs       # SIGWINCH, SIGHUP handlers
    terminal.rs      # raw mode RAII guard, terminal size queries
```

---

## Verification (End-to-End)

| Scenario | Expected |
|---|---|
| `ls -la` | Executes immediately, no LLM call |
| `show me all .rs files` | LLM → `find . -name "*.rs"` → confirm → executes |
| `find the largest file` | `find` in PATH → routes as shell → `find: ...` usage error from shell |
| Ollama not running | Warning at startup; NL input forwarded to shell |
| `vim` | Passes through; no intercept; resize works |
| Ctrl+C during LLM stream | Cancels; returns to prompt; no corruption |
| `rm -rf ~` from LLM | Destructive warning shown before confirm prompt |
| `exit` | Child zsh exits; wrapper exits same code; terminal restored |

---

## Dependencies (pinned at plan time)

| Crate | Version | Purpose |
|---|---|---|
| `portable-pty` | 0.8 | pty master/slave, SIGWINCH, resize |
| `tokio` | 1.x | async runtime |
| `reqwest` | 0.12 | Ollama HTTP client (streaming) |
| `serde_json` | 1.x | JSON serialization |
| `nix` | 0.29 | termios raw mode, ioctl |
| `clap` | 4.x | CLI arg parsing |
| `tempfile` | 3.x | editor tempfile for `e` confirm path |
