# Natural Language Terminal — Project Outline

## What We're Building

A Rust binary that wraps your existing shell (zsh/bash). Once running, you use your terminal exactly as normal — except you can also just type plain English. It figures out which is which automatically, routes English to a local LLM, gets a shell command back, shows it to you, and runs it on confirmation.

No cloud. No API keys. No mode switching. It just works.

---

## The Core Idea

The key insight is that we don't need to classify natural language vs shell commands ourselves. We hand the input to `zsh -n` (syntax check, no execution). If it's valid shell, run it. If it errors, it's probably English — send it to the LLM.

This means zero heuristics, zero ML for the routing layer. The shell itself is the classifier.

There's one known edge case: English that accidentally looks like a valid shell command (e.g. `find me the largest file` — `find` is a real command). The fix is a second pass: if a command executes but returns a usage error or "not found", offer to re-route it to the LLM. Good enough for v1.

---

## How It Works End to End

1. User opens terminal, runs the binary (or it's set as their default shell)
2. Binary forks a real shell as a child process via a pseudoterminal (pty)
3. User types something and hits enter
4. Binary intercepts the line before it reaches the shell
5. Runs `zsh -n` on it
6. **Valid syntax** → forwards directly to the child shell, behaves exactly as normal
7. **Syntax error** → sends to local LLM with a system prompt asking for a shell command back
8. LLM returns a command
9. Binary displays it, asks for confirmation
10. User confirms → executes it. User edits → edits it. User rejects → drops it.

---

## The Stack

**Language:** Rust. Chosen because pty handling and low-level terminal I/O is where Rust's performance and safety actually matter, and we want this to be snappy — LLM latency is already the bottleneck, the wrapper should add nothing.

**LLM backend:** Ollama running locally. Exposes an OpenAI-compatible HTTP API at `localhost:11434`. Swappable — same interface works for anything Ollama supports (Llama, Qwen, Mistral, etc.). Eventually could also route to Apple's Foundation Model via `afm-cli` as a subprocess.

**Shell integration:** pty-based wrapper, not a shell plugin. Means it works regardless of what shell you use and doesn't require sourcing anything into your config.

---

## Phases

**Phase 1 — The Wrapper**
Get the pty working. The binary forks zsh, pipes stdin/stdout through itself, and the user sees no difference from a normal terminal session. Interactive programs (vim, ssh, python REPL) pass through without interference.

**Phase 2 — The Router**
Intercept each line before it hits the shell. Run the `zsh -n` check. Branch on the result. At this point, failed lines just get printed back as errors (no LLM yet) — but the interception and routing logic is solid.

**Phase 3 — LLM Integration**
Hook up Ollama. Failed lines go to the LLM with a tight system prompt: "you are a shell command translator, return only the command, nothing else." Stream the response back. Display it with a confirm/edit/reject prompt before execution.

**Phase 4 — UX Polish**
Streaming output so the command appears token by token. Good error messages. A `--dry-run` flag that shows what the LLM would have done without executing. Shell history integration so LLM-generated commands appear in `history` like normal ones.

**Phase 5 — Apple Foundation Model**
Investigate routing to the on-device 3B model via `afm-cli` instead of Ollama for users who want zero dependencies. Lower quality than a 7B Ollama model but instant, no setup.

---

## What We're Not Building (Yet)

- A GUI or TUI — pure terminal
- Voice input
- Multi-turn conversation in the shell (one-shot translation only, for now)
- Any cloud LLM integration
- A VSCode extension or IDE plugin

---

## Open Questions

- **Default confirmation behavior** — should it auto-execute low-risk commands (like `ls` variants) and only confirm destructive ones? Or always confirm? Probably always confirm for v1, opt-in auto-exec later.
- **Context passing** — should the LLM know your current directory, recent history, env vars? Richer context = better commands, but also more tokens per request. Worth experimenting with.
- **Model recommendation** — what's the best Ollama model for shell command translation at ~7B scale? Qwen2.5-Coder:7B is the current hypothesis. Needs benchmarking against a small eval set of NL→command pairs.
- **Name** — needs one.