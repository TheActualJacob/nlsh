# nlsh — Natural Language Shell

Type plain English at your shell prompt. Get a shell command back, confirm it, and run it — all without leaving the terminal.

```
$ list all files modified today
⟳ thinking...
  find . -maxdepth 1 -newer $(date -d today +%Y-%m-%d) -type f
[y] run  [e] edit  [n] cancel
```

No cloud. No API keys. No mode switching. Runs entirely on-device — either via Apple Foundation Models or a local Ollama model.

## Requirements

- macOS 26 or later
- **Apple Intelligence** (System Settings → Apple Intelligence), or
- **Ollama** — nlsh will offer to install it automatically during setup

## Install

### One-liner (build from source)

Unfortunately Requires [Rust](https://rustup.rs) and Xcode Command Line Tools due to apple's HORRIBLE notarization systems.

```sh
git clone https://github.com/TheActualJacob/nlsh.git && cd nlsh/nlsh && cargo build --release && sudo ./target/release/nlsh --install
```

Then set as your default shell:

```sh
chsh -s /usr/local/bin/nlsh
```

### One-liner (pre-built, when a release is available)

```sh
curl -fsSL https://github.com/TheActualJacob/nlsh/releases/latest/download/install.sh | sh
```

## First launch

The first time you run nlsh, a setup screen appears:

```
  ╭────────────────────────────────────────────╮
  │         nlsh · model setup                 │
  ╰────────────────────────────────────────────╯

  Choose an AI backend:

  ▶ 1  Apple Intelligence   on-device, no download
    2  Ollama local model   ~986 MB download

  [↑↓ / 1-2] navigate   [Enter] confirm   [q] quit
```

- **Apple Intelligence** — uses the on-device model, no download needed
- **Ollama** — downloads `qwen2.5-coder:1.5b` (~986 MB); nlsh installs Ollama automatically via Homebrew if it isn't already present

At the end of setup you'll be asked whether to set nlsh as your default shell.

Run `nlsh --setup` at any time to switch backends or re-configure.

## Usage

From here, use it exactly like zsh. Shell commands run normally. When you type something that isn't a shell command, nlsh routes it to the model:

```
$ show disk usage for each folder here, sorted largest first
⟳ thinking...
  du -sh */ | sort -rh
[y] run  [e] edit  [n] cancel > y

# command runs...
```

### Confirmation prompt

| Key | Action |
|-----|--------|
| `y` or `Enter` | Run the command |
| `e` | Open the command in `$EDITOR` to modify before running |
| `n` or `Esc` | Cancel |

### Flags

| Flag | Description |
|------|-------------|
| `--setup` | Re-run model selection / backend configuration |
| `--dry-run` | Print the generated command but don't run it |
| `--no-hist` | Prefix generated commands with a space (hides them from history if `HIST_IGNORE_SPACE` is set in `.zshrc`) |

## How it works

nlsh wraps zsh in a pseudoterminal. Every line you type is classified before being sent to the shell:

- **Recognized command** (exists in `$PATH`, is a builtin, alias, or function) → forwarded directly to zsh.
- **Unrecognized input** → sent to the configured model for translation into a shell command.

When a TUI application starts (vim, less, htop, ssh), nlsh detects the alternate screen and switches to full passthrough mode — every keystroke goes straight to the pty with zero interception.

## Troubleshooting

**"Apple Intelligence unavailable — NL routing disabled"**

- macOS 26+ required
- Apple Intelligence must be enabled: System Settings → Apple Intelligence & Siri
- Your device must be eligible (M-series Mac or A17 Pro iPhone/iPad and later)
- Switch to Ollama with `nlsh --setup` if Apple Intelligence isn't available

**"Ollama unavailable — NL routing disabled"**

Ollama isn't running. Start it with `ollama serve`, then open a new nlsh session. Or switch backends with `nlsh --setup`.

**Generated commands aren't going into shell history**

Pass `--no-hist` and add `setopt HIST_IGNORE_SPACE` to your `~/.zshrc`.

## Uninstall

```sh
sudo rm /usr/local/bin/nlsh /usr/local/bin/nlsh-model
# Remove /usr/local/bin/nlsh from /etc/shells
chsh -s /bin/zsh   # if you set nlsh as your default shell
```

## License

MIT
