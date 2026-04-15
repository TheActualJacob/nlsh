# nlsh — Natural Language Shell

A Rust binary that wraps zsh via a pseudoterminal. Type plain English; get a shell command back, confirm, and run it. No cloud. No API keys. No mode switching.

## Project layout

```
nlsh-model/             Swift package — Apple Foundation Models shim
  Package.swift
  Sources/nlsh-model/
    main.swift          --check mode + inference mode; prints response to stdout

nlsh/                   Rust crate
  build.rs              Compiles nlsh-model Swift package; bakes shim path into binary
  src/
    main.rs             CLI args (clap), startup checks, thread orchestration
    terminal.rs         RawMode RAII guard, get_terminal_size()
    signals.rs          SIGWINCH / SIGHUP handlers + CHILD_EXITED flag
    pty.rs              PtySession: opens pty pair, spawns child zsh
    classify.rs         LineKind classifier — uses `zsh type -a` on first token
    prompt.rs           ShellContext + build_prompt() for Foundation Models
    llm.rs              Subprocess client for nlsh-model shim (no network deps)
    parser.rs           LLM output cleaner (strips fences, explanation lines)
    safety.rs           Destructive command pattern detection
    confirm.rs          Single-keypress confirm/edit/cancel UI
    intercept.rs        Buffered line editor, escape-sequence handling, routing
```

## Build

```sh
cd nlsh
cargo build           # debug (also builds nlsh-model Swift package via build.rs)
cargo build --release # release
cargo test            # 18 unit tests
```

Requires:
- Rust stable ≥ 1.77
- `swift` CLI (ships with Xcode Command Line Tools on macOS 26+)
- macOS 26+ with Apple Intelligence enabled (System Settings → Apple Intelligence)

## Run

```sh
cargo run             # opens a zsh session with NL routing
cargo run -- --dry-run  # shows generated commands without executing
```

If Apple Intelligence is unavailable at startup, NL routing is disabled and nlsh behaves as a transparent shell wrapper.

## Install as login shell

```sh
./target/release/nlsh --install
# then:
chsh -s /usr/local/bin/nlsh
```

`--install` copies both `nlsh` and `nlsh-model` to `/usr/local/bin/`.

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

### LLM integration — Swift shim architecture

`llm.rs` spawns `nlsh-model` as a subprocess:

- **`--check` mode**: shim exits 0 if `SystemLanguageModel.default.availability == .available`, exits 1 otherwise.
- **Inference mode**: shim reads the full prompt from stdin, calls `LanguageModelSession.streamResponse(to:)`, accumulates snapshots, and prints the full response to stdout followed by a newline.
- **Rust side (v1)**: uses `wait_with_output()` — waits for the full response, then prints it. No streaming UX; the on-device model is fast enough that this is acceptable.

Shim discovery order in `llm::shim_path()`:
1. Sibling of current executable (used after `nlsh --install`)
2. Build-time path baked in by `build.rs` via `NLSH_MODEL_BUILD_PATH` env var (used with `cargo run`)
3. `nlsh-model` on `$PATH` (last-ditch fallback)

### Child exit detection

When the output thread gets `EIO`/`EOF` from the pty master read (child shell exited), it sets `CHILD_EXITED` and sends `SIGHUP` to the wrapper process. This interrupts the blocking `libc::read` in the intercept loop (returns `EINTR`), which checks the flag and exits cleanly.

## Known limitations (v1)

- **v1 is non-streaming**: nlsh waits for the full response before displaying it. A thinking indicator (`⟳ thinking...`) is shown while waiting.
- **Multi-byte UTF-8 characters** typed at the prompt: only ASCII bytes are correctly echoed and backspaced. UTF-8 input works for shell-routed lines but visual echo may be off.
- **--no-hist** requires `setopt HIST_IGNORE_SPACE` in the user's `.zshrc` to take effect.
- macOS only. Linux pty behavior differs slightly (not tested).

## Open questions (from rough plan, still open)

- Context enrichment: passing recent history entries alongside `$PWD` improves quality but increases token count per request.
- Streaming v2: shim can write each delta followed by `\n`; Rust side reads line-by-line for progressive display.

## Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `portable-pty` | 0.8 | pty master/slave, SIGWINCH, resize |
| `nix` | 0.27 | termios raw mode (tcgetattr/tcsetattr/cfmakeraw) |
| `clap` | 4.x | CLI arg parsing |
| `tempfile` | 3.x | Editor tempfile for `e` confirm path |
| `anyhow` | 1.x | Error propagation |
| `libc` | 0.2 | Raw ioctl, read(), kill() |

No network dependencies — inference runs entirely on-device via Apple Foundation Models.
