# Apple Foundation Models Backend — Implementation Plan

## Goal

Replace the Ollama HTTP backend with a bundled Swift command-line shim that calls Apple's Foundation Models framework, eliminating the Ollama dependency entirely.

---

## Constraints

- macOS 26+ required (Darwin 25+). User's system: Darwin 25.5.0. ✓
- Apple Intelligence must be enabled on device (System Settings → Apple Intelligence). User precondition; not installer-controlled.
- `FoundationModels` framework: Swift only — Rust cannot call it directly. Requires a Swift shim subprocess.
- `ResponseStream<String>` yields **cumulative snapshots**, not deltas. Shim must compute deltas before printing to Rust's stdout.
- `swift` CLI must be available: ships with Xcode Command Line Tools on macOS 26.
- Rust interface (`LlmError`, `check_available()`, `generate()`) must stay identical — `intercept.rs` must not change.
- Remove `reqwest`, `serde`, `serde_json` from `Cargo.toml`; they are only used by `llm.rs`.

---

## Unknowns / Risks

- **Entitlement requirement for CLI tools**: Apple docs describe entitlements in the context of App Store apps. Unknown whether a locally built, ad-hoc–signed command-line binary can call `LanguageModelSession` without a provisioning profile. If it cannot, the shim will receive a runtime error from the framework. Resolution: test immediately after Step 2; if blocked, try `codesign --sign - nlsh-model` (ad-hoc) or full developer ID signing.
- **`@MainActor` isolation on `LanguageModelSession`**: `streamResponse` returns `sending` — may require main-actor context. Mitigated by using `@main struct` with `static func main() async throws` which runs on the main actor.
- **`ResponseStream<String>` element type**: context7 docs confirm it yields cumulative `String` snapshots. Shim tracks `previous.endIndex` and prints only the suffix since the last snapshot.
- **`swift build` in `build.rs`**: `cargo build` will fail if `swift` is not on `$PATH`. `build.rs` must emit a warning and provide a fallback shim path rather than hard-failing.
- **Shim binary location at runtime**: for `cargo run` (dev), shim is at `../nlsh-model/.build/release/nlsh-model`. For release install (`nlsh --install`), shim must be copied alongside the `nlsh` binary. `llm.rs` must search both locations.

---

## Steps

### Step 1 — Create Swift package skeleton

Create `nlsh-model/` adjacent to `nlsh/` (i.e., at project root level).

**`nlsh-model/Package.swift`**:
```swift
// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "nlsh-model",
    platforms: [.macOS(.v26)],
    targets: [
        .executableTarget(
            name: "nlsh-model",
            path: "Sources/nlsh-model"
        )
    ]
)
```

### Step 2 — Write Swift shim (`nlsh-model/Sources/nlsh-model/main.swift`)

Responsibilities:
1. `--check` flag: print `available` / `unavailable:<reason>` to stdout; exit 0 if available, 1 if not.
2. Normal mode: read full prompt from stdin (until EOF); call `LanguageModelSession.streamResponse(to:options:)`; print deltas to stdout as they arrive; exit 0 on success.
3. Any error: print message to stderr; exit 2.

```swift
import Foundation
import FoundationModels

@main
struct NlshModel {
    static func main() async {
        let args = CommandLine.arguments

        // ── Availability check mode ──────────────────────────────────────────
        if args.contains("--check") {
            let model = SystemLanguageModel.default
            switch model.availability {
            case .available:
                print("available")
                exit(0)
            case .unavailable(.deviceNotEligible):
                print("unavailable:deviceNotEligible")
                exit(1)
            case .unavailable(.appleIntelligenceNotEnabled):
                print("unavailable:appleIntelligenceNotEnabled")
                exit(1)
            case .unavailable(.modelNotReady):
                print("unavailable:modelNotReady")
                exit(1)
            case .unavailable(let other):
                print("unavailable:\(other)")
                exit(1)
            }
        }

        // ── Inference mode ───────────────────────────────────────────────────
        guard case .available = SystemLanguageModel.default.availability else {
            fputs("nlsh-model: model not available\n", stderr)
            exit(1)
        }

        // Read full prompt from stdin.
        var promptText = ""
        while let line = readLine(strippingNewline: false) {
            promptText += line
        }
        guard !promptText.isEmpty else {
            fputs("nlsh-model: empty prompt\n", stderr)
            exit(2)
        }

        let session = LanguageModelSession()
        let stream = session.streamResponse(to: Prompt(promptText))

        var previous = ""
        do {
            for try await snapshot in stream {
                // ResponseStream<String> yields cumulative snapshots — emit only
                // the new suffix since the last snapshot.
                let delta = String(snapshot.dropFirst(previous.count))
                if !delta.isEmpty {
                    print(delta, terminator: "")
                    fflush(stdout)
                }
                previous = snapshot
            }
        } catch {
            fputs("nlsh-model: generation error: \(error)\n", stderr)
            exit(2)
        }

        print() // final newline
        exit(0)
    }
}
```

**Verify Step 2** before proceeding:
```sh
cd nlsh-model
swift build -c release 2>&1
.build/release/nlsh-model --check   # must print "available" and exit 0
echo "list files by size" | .build/release/nlsh-model   # must stream a command
```

If `--check` fails with a framework error (not "unavailable:*"), the entitlement issue is triggered. Fix: `codesign --sign - .build/release/nlsh-model` and retry.

### Step 3 — Add `nlsh/build.rs`

Compiles the Swift package during `cargo build` and bakes the shim path into the binary as a compile-time env var.

```rust
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let shim_dir = manifest_dir.parent().unwrap().join("nlsh-model");
    let shim_release = shim_dir.join(".build/release/nlsh-model");

    println!("cargo:rerun-if-changed=../nlsh-model/Sources/nlsh-model/main.swift");
    println!("cargo:rerun-if-changed=../nlsh-model/Package.swift");

    let ok = Command::new("swift")
        .args(["build", "-c", "release"])
        .current_dir(&shim_dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok {
        println!("cargo:rustc-env=NLSH_MODEL_BUILD_PATH={}", shim_release.display());
    } else {
        // Emit warning; don't hard-fail the Rust build.
        // At runtime, check_available() will return false and NL routing is disabled.
        println!("cargo:warning=nlsh-model Swift shim build failed — NL routing will be disabled");
        println!("cargo:rustc-env=NLSH_MODEL_BUILD_PATH=");
    }
}
```

### Step 4 — Rewrite `nlsh/src/llm.rs`

Remove: all reqwest/serde/JSON code, `GenerateRequest`, `GenerateOptions`, `GenerateChunk`, `client()`.

Keep: `LlmError` (same variants, update display strings), `check_available()`, `generate()`.

New implementation:

```rust
use anyhow::Result;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const BUILD_PATH: &str = env!("NLSH_MODEL_BUILD_PATH");

#[derive(Debug)]
pub enum LlmError {
    Unavailable,
    Other(anyhow::Error),
}
// impl Display: update "ollama not reachable" → "Apple Intelligence not available"

fn shim_path() -> PathBuf {
    // 1. Prefer sibling of current executable (release install).
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.parent().unwrap_or(exe.as_path()).join("nlsh-model");
        if candidate.exists() {
            return candidate;
        }
    }
    // 2. Fall back to build-time path (cargo run / dev).
    if !BUILD_PATH.is_empty() {
        let p = PathBuf::from(BUILD_PATH);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("nlsh-model") // last-ditch: hope it's on $PATH
}

pub fn check_available() -> bool {
    Command::new(shim_path())
        .arg("--check")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn generate(prompt: &str) -> Result<String, LlmError> {
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

    // Write prompt to shim's stdin, then close it.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).ok();
    }

    // Stream stdout tokens to the terminal.
    print!("\r\x1b[2K  \x1b[36m⟳\x1b[0m ");
    std::io::stdout().flush().ok();

    let mut full = String::new();
    if let Some(stdout) = child.stdout.take() {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(chunk) if !chunk.is_empty() => {
                    print!("{chunk}");
                    std::io::stdout().flush().ok();
                    full.push_str(&chunk);
                    full.push('\n');
                }
                Ok(_) => {}
                Err(e) => return Err(LlmError::Other(e.into())),
            }
        }
    }

    let status = child.wait().map_err(|e| LlmError::Other(e.into()))?;
    if !status.success() {
        let code = status.code().unwrap_or(-1);
        if code == 1 {
            return Err(LlmError::Unavailable);
        }
        return Err(LlmError::Other(anyhow::anyhow!("nlsh-model exited {code}")));
    }

    println!();
    Ok(full.trim_end_matches('\n').to_string())
}
```

**Note on stdout streaming from shim**: The shim prints deltas (not line-by-line). `BufReader::lines()` will only deliver data once a `\n` is encountered. For true token streaming, use `read()` byte-by-byte or use `BufReader::read_until(b'\n', ...)`. The shim ends with a final `print()` that flushes the buffer, so the full response is delivered at that point.

**Alternative (simpler, v1)**: Use `child.wait_with_output()` instead of streaming stdout — waits for full response, then prints it. No streaming UX but eliminates the newline-buffering issue. Implement this for v1; stream later.

**Decision**: Implement non-streaming v1 (wait_with_output). The on-device 3B model is fast; streaming adds complexity for minimal benefit here. If streaming is wanted later, the shim can write each delta followed by `\n` and the Rust side reads line-by-line.

Revised `generate()` using `wait_with_output()`:

```rust
pub fn generate(prompt: &str) -> Result<String, LlmError> {
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

    // Show thinking indicator while waiting.
    print!("\r\x1b[2K  \x1b[36m⟳\x1b[0m thinking...");
    std::io::stdout().flush().ok();

    let output = child.wait_with_output().map_err(|e| LlmError::Other(e.into()))?;

    print!("\r\x1b[2K"); // clear thinking line
    std::io::stdout().flush().ok();

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        return if code == 1 {
            Err(LlmError::Unavailable)
        } else {
            Err(LlmError::Other(anyhow::anyhow!("nlsh-model exited {code}")))
        };
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
```

### Step 5 — Update `nlsh/Cargo.toml`

Remove:
```toml
reqwest = { version = "0.12", features = ["json", "blocking"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

These are only used by `llm.rs`. Verify no other modules import them.

### Step 6 — Update string literals in `nlsh/src/main.rs`

Line ~55: change startup check message.

Old:
```rust
eprintln!("[nlsh: ollama unreachable — NL routing disabled]");
```

New:
```rust
eprintln!("[nlsh: Apple Intelligence unavailable — NL routing disabled]");
```

### Step 7 — Update `nlsh/src/intercept.rs` error string

Line ~215: change `LlmError::Unavailable` message.

Old:
```rust
"\r\x1b[2K  \x1b[33m[nlsh: ollama not running — forwarding as shell command]\x1b[0m"
```

New:
```rust
"\r\x1b[2K  \x1b[33m[nlsh: Apple Intelligence unavailable — forwarding as shell command]\x1b[0m"
```

### Step 8 — Update `nlsh --install` to copy the shim

`main.rs::cmd_install()`: after copying `nlsh` binary to `/usr/local/bin/nlsh`, also copy the shim.

Add after the existing copy:
```rust
let shim_src = crate::llm::shim_path(); // make shim_path() pub
let shim_target = std::path::Path::new("/usr/local/bin/nlsh-model");
if shim_src.exists() {
    std::fs::copy(&shim_src, shim_target).ok();
    // chmod +x
}
```

### Step 9 — Update `.gitignore`

Add `nlsh-model/.build/` to root `.gitignore`.

### Step 10 — Update `CLAUDE.md`

Remove Ollama references. Document:
- Swift shim architecture (`nlsh-model/`)
- Build requirement (`swift` CLI / Xcode Command Line Tools)
- Foundation Models availability precondition
- Shim discovery logic (sibling exe → build-time path)
- Streaming note (v1 non-streaming; shim prints full response)
- Remove `reqwest`/`serde`/`serde_json` from dependency table; add note that no network dependencies remain

---

## Verification

| Check | Signal |
|---|---|
| Shim builds | `cd nlsh-model && swift build -c release` exits 0 |
| Shim availability check | `.build/release/nlsh-model --check` prints `available`, exits 0 |
| Shim inference | `echo "list files by size" \| .build/release/nlsh-model` prints a shell command |
| Rust builds | `cargo build` in `nlsh/` exits 0, zero warnings |
| Rust tests | `cargo test` 18/18 pass (no test changes needed) |
| NL routing works | `cargo run`, type `show me disk usage` → confirmation prompt appears |
| No Ollama dependency | `lsof -i :11434` shows nothing; nlsh works with Ollama not running |
| Install copies shim | `nlsh --install` copies both `nlsh` and `nlsh-model` to `/usr/local/bin/` |
| Unavailable path | Disable Apple Intelligence in Settings; `cargo run` prints `[nlsh: Apple Intelligence unavailable — NL routing disabled]`; shell still works |

---

## Files Changed

| Path | Action |
|---|---|
| `nlsh-model/Package.swift` | create |
| `nlsh-model/Sources/nlsh-model/main.swift` | create |
| `nlsh/build.rs` | create |
| `nlsh/src/llm.rs` | rewrite (remove reqwest/HTTP; add subprocess shim) |
| `nlsh/src/main.rs` | modify (1 string, install step) |
| `nlsh/src/intercept.rs` | modify (1 string) |
| `nlsh/Cargo.toml` | modify (remove 3 deps) |
| `.gitignore` | modify (add nlsh-model/.build/) |
| `CLAUDE.md` | modify (remove Ollama refs, document new arch) |
