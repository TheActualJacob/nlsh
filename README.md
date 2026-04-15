# nlsh — Natural Language Shell

Type plain English at your shell prompt. Get a shell command back, confirm it, and run it — all without leaving the terminal.

```
$ list all files modified today
⟳ thinking...
  find . -maxdepth 1 -newer $(date -d today +%Y-%m-%d) -type f
[y] run  [e] edit  [n] cancel
```

No cloud. No API keys. No mode switching. Inference runs entirely on-device via Apple Foundation Models.

## Requirements

- macOS 26 or later
- Apple Intelligence enabled (System Settings → Apple Intelligence & Siri)

## Install

### Download pre-built binaries (recommended)

```sh
# Download both binaries
curl -Lo nlsh https://github.com/TheActualJacob/nlsh/releases/latest/download/nlsh
curl -Lo nlsh-model https://github.com/TheActualJacob/nlsh/releases/latest/download/nlsh-model

# Install
chmod +x nlsh nlsh-model
sudo mv nlsh nlsh-model /usr/local/bin/

# Register as a valid shell
echo '/usr/local/bin/nlsh' | sudo tee -a /etc/shells
```

### (Optional) Set as your default shell

```sh
chsh -s /usr/local/bin/nlsh
```

You can always switch back with `chsh -s /bin/zsh`.

## Usage

Launch nlsh (or open a new terminal if you set it as your default shell):

```sh
nlsh
```

From here, use it exactly like zsh. Shell commands run normally. When you type something that isn't a shell command, nlsh routes it to the on-device model:

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
| `--dry-run` | Print the generated command but don't run it |
| `--no-hist` | Prefix generated commands with a space (hides them from history if `HIST_IGNORE_SPACE` is set in `.zshrc`) |

```sh
nlsh --dry-run     # preview mode — nothing executes
nlsh --no-hist     # generated commands stay out of shell history
```

## How it works

nlsh wraps zsh in a pseudoterminal. Every line you type is classified before being sent to the shell:

- **Recognized command** (exists in `$PATH`, is a builtin, alias, or function) → forwarded directly to zsh.
- **Unrecognized input** → sent to the on-device Apple Foundation Models for translation into a shell command.

When a TUI application starts (vim, less, htop, ssh), nlsh detects the alternate screen and switches to full passthrough mode — every keystroke goes straight to the pty with zero interception.

## Troubleshooting

**"Apple Intelligence unavailable — NL routing disabled"**

NL routing is silently skipped and nlsh behaves as a plain zsh wrapper. Fix the underlying issue:
- macOS 26+ required
- Apple Intelligence must be enabled: System Settings → Apple Intelligence & Siri → turn on Apple Intelligence
- Your device must be eligible (M-series Mac or A17 Pro iPhone/iPad and later)

**Permission denied during install**

Run the `sudo mv` step, or if that fails, try:
```sh
sudo install -m 755 nlsh nlsh-model /usr/local/bin/
```

**Generated commands aren't going into shell history**

Pass `--no-hist` and add `setopt HIST_IGNORE_SPACE` to your `~/.zshrc`.

## Uninstall

```sh
sudo rm /usr/local/bin/nlsh /usr/local/bin/nlsh-model
# Edit /etc/shells and remove the /usr/local/bin/nlsh line
# If you changed your default shell:
chsh -s /bin/zsh
```

## Build from source

Requires Rust stable ≥ 1.77 and Xcode Command Line Tools.

```sh
git clone https://github.com/TheActualJacob/nlsh.git
cd nlsh/nlsh
cargo build --release
sudo cp target/release/nlsh /usr/local/bin/nlsh
sudo cp ../nlsh-model/.build/release/nlsh-model /usr/local/bin/nlsh-model
echo '/usr/local/bin/nlsh' | sudo tee -a /etc/shells
```

## License

MIT
