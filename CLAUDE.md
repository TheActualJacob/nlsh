# nlsh — Natural Language Shell

A Rust binary that wraps zsh via a pseudoterminal. Type plain English; get a shell command back, confirm, and run it. No cloud. No API keys. No mode switching.

## Project layout

```
nlsh/                   Rust crate
  src/
    main.rs             CLI args (clap), startup checks, thread orchestration
    terminal.rs         RawMode RAII guard, get_terminal_size()
    signals.rs          SIGWINCH / SIGHUP handlers + CHILD_EXITED flag
    pty.rs              PtySession: opens pty pair, spawns child zsh
    classify.rs         LineKind classifier — uses `zsh type -a` on first token
    prompt.rs           ShellContext + build_prompt() for Ollama
    llm.rs              Blocking Ollama /api/generate client with streaming
    parser.rs           LLM output cleaner (strips fences, explanation lines)
    safety.rs           Destructive command pattern detection
    confirm.rs          Single-keypress confirm/edit/cancel UI
    intercept.rs        Buffered line editor, escape-sequence handling, routing
```

## Build

```sh
cd nlsh
cargo build           # debug
cargo build --release # release
cargo test            # 18 unit tests
```

Requires Rust stable ≥ 1.77.

## Run

```sh
cargo run             # opens a zsh session with NL routing
cargo run -- --dry-run  # shows generated commands without executing
```

**Ollama must be running** (`ollama serve`). Default model: `qwen2.5-coder:7b`. Pull it with:
```sh
ollama pull qwen2.5-coder:7b
```

If Ollama is unreachable at startup, NL routing is disabled and nlsh behaves as a transparent shell wrapper.

## Install as login shell

```sh
./target/release/nlsh --install
# then:
chsh -s /usr/local/bin/nlsh
```

## Architecture

### Threading model

| Thread | Responsibility |
|---|---|
| Main | stdin intercept loop (blocking `libc::read`) |
| Output | pty master → stdout; sets passthrough flag on alternate screen sequences |
| SIGWINCH | polls `SIGWINCH_RECEIVED` every 50ms; calls `master.resize()` |

### Input classification

`classify(line)` runs `zsh -c 'type -a "$1"'` on the first token. If the first word is not a command/builtin/alias/function in `$PATH`, the line is routed to the LLM. This replaces the original `zsh -n` approach from the rough plan, which only checks syntax (not command existence) and would incorrectly classify most English sentences as valid shell.

### Passthrough mode

When the output thread detects `\x1b[?1049h` (alternate screen enter — vim, less, htop, ssh), it sets `passthrough = true`. Every stdin byte is forwarded directly to the pty master without buffering or classification. Cleared on `\x1b[?1049l`.

### LLM integration

Uses Ollama's native `/api/generate` endpoint (not `/v1/chat/completions`, which is marked experimental). Streams tokens token-by-token, prints them live, then passes the full response through `parser::clean_llm_output()` before the confirm prompt.

### Child exit detection

When the output thread gets `EIO`/`EOF` from the pty master read (child shell exited), it sets `CHILD_EXITED` and sends `SIGHUP` to the wrapper process. This interrupts the blocking `libc::read` in the intercept loop (returns `EINTR`), which checks the flag and exits cleanly.

## Known limitations (v1)

- **Ctrl+C during LLM streaming** does not cancel the in-flight request. The 30s reqwest timeout is the escape hatch.
- **Multi-byte UTF-8 characters** typed at the prompt: only ASCII bytes are correctly echoed and backspaced. UTF-8 input works for shell-routed lines but visual echo may be off.
- **--no-hist** requires `setopt HIST_IGNORE_SPACE` in the user's `.zshrc` to take effect.
- macOS only. Linux pty behavior differs slightly (not tested).

## Open questions (from rough plan, still open)

- Best Ollama model at ~7B scale for shell translation — `qwen2.5-coder:7b` is the hypothesis; needs benchmarking against a fixed eval set.
- Context enrichment: passing recent history entries alongside `$PWD` improves quality but increases token count per request.

## Phase 5 (deferred)

Apple Foundation Models on-device integration. Prerequisite: a small Swift shim binary calling the `FoundationModels` framework. `afm-cli` (third-party) is not a target — it's unstable and adds an uncontrolled dependency.

## Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `portable-pty` | 0.8 | pty master/slave, SIGWINCH, resize |
| `nix` | 0.27 | termios raw mode (tcgetattr/tcsetattr/cfmakeraw) |
| `reqwest` | 0.12 (blocking) | Ollama HTTP client |
| `serde` / `serde_json` | 1.x | JSON for Ollama API |
| `clap` | 4.x | CLI arg parsing |
| `tempfile` | 3.x | Editor tempfile for `e` confirm path |
| `anyhow` | 1.x | Error propagation |
| `libc` | 0.2 | Raw ioctl, read(), kill() |
