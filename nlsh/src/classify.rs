#[derive(Debug, PartialEq)]
pub enum LineKind {
    Shell,
    NaturalLanguage,
}

/// Classify a line of input.
///
/// Uses `zsh type -a <first_token>` to determine whether the first word is
/// a known command, builtin, alias, or function.  If it is, the input is
/// treated as a shell command.  If not, it is routed to the LLM.
pub fn classify(line: &str) -> LineKind {
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return LineKind::Shell;
    }

    let first_char = trimmed.chars().next().unwrap();

    // History expansion, comments — definitely shell.
    if first_char == '!' || first_char == '#' {
        return LineKind::Shell;
    }

    let first_token = trimmed.split_whitespace().next().unwrap_or("");

    // Variable assignment (TOKEN=value) or explicit path (/usr/bin/foo, ./foo).
    if first_token.contains('=') || first_token.contains('/') {
        return LineKind::Shell;
    }

    // Subshell / command grouping.
    if first_char == '(' || first_char == '{' {
        return LineKind::Shell;
    }

    // Check whether the first token is a known command via `zsh type -a`.
    // Stdin/stdout/stderr are nulled so the subprocess doesn't interact with
    // our raw-mode terminal.
    let result = std::process::Command::new("zsh")
        .args(["-c", "type -a \"$1\" >/dev/null 2>&1", "--", first_token])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match result {
        Ok(status) if status.success() => {
            // The first token is a known command, but the rest of the line may
            // still be natural language (e.g. "find my ip address", "what is
            // my username"). If the arguments look like plain English rather
            // than shell usage, route to NL anyway.
            if looks_like_nl_args(trimmed) {
                LineKind::NaturalLanguage
            } else {
                LineKind::Shell
            }
        }
        _ => LineKind::NaturalLanguage,
    }
}

/// Returns true if the arguments following the first token look like natural
/// language rather than shell usage.
///
/// Shell usage has flags (`-x`, `--foo`), paths (`/usr`, `./file`, `~`),
/// globs (`*.rs`), redirects, or quoted strings.  If none of those are
/// present and there are at least two additional words, the line is almost
/// certainly an English sentence whose first word happens to be a command.
fn looks_like_nl_args(line: &str) -> bool {
    let mut tokens = line.split_whitespace();
    let _ = tokens.next(); // skip first token (already known to be a command)
    let args: Vec<&str> = tokens.collect();

    // Need at least two more words to distinguish "find ." from "find my ip".
    if args.len() < 2 {
        return false;
    }

    // Any shell-ish token → treat as shell.
    let shell_chars = |s: &str| {
        s.starts_with('-')          // flag
            || s.starts_with('/')   // absolute path
            || s.starts_with('.')   // relative path or glob
            || s.starts_with('~')   // home dir
            || s.contains('*')      // glob
            || s.contains('?')      // glob
            || s.contains('=')      // var=val
            || s.starts_with('"')
            || s.starts_with('\'')
    };

    if args.iter().any(|a| shell_chars(a)) {
        return false;
    }

    // All remaining tokens are plain lowercase/uppercase words — NL.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_shell() {
        assert_eq!(classify(""), LineKind::Shell);
        assert_eq!(classify("   "), LineKind::Shell);
    }

    #[test]
    fn history_expansion_is_shell() {
        assert_eq!(classify("!!"), LineKind::Shell);
        assert_eq!(classify("!git"), LineKind::Shell);
    }

    #[test]
    fn comment_is_shell() {
        assert_eq!(classify("# comment"), LineKind::Shell);
    }

    #[test]
    fn assignment_is_shell() {
        assert_eq!(classify("FOO=bar"), LineKind::Shell);
        assert_eq!(classify("FOO=bar baz"), LineKind::Shell);
    }

    #[test]
    fn explicit_path_is_shell() {
        assert_eq!(classify("/usr/bin/env python"), LineKind::Shell);
        assert_eq!(classify("./myscript.sh"), LineKind::Shell);
    }

    #[test]
    fn ls_is_shell() {
        assert_eq!(classify("ls -la"), LineKind::Shell);
    }

    #[test]
    fn unknown_is_nl() {
        assert_eq!(classify("show me disk usage"), LineKind::NaturalLanguage);
    }

    #[test]
    fn find_with_flags_is_shell() {
        assert_eq!(classify("find . -name '*.rs'"), LineKind::Shell);
        assert_eq!(classify("find /tmp -mtime -1"), LineKind::Shell);
    }

    #[test]
    fn find_english_sentence_is_nl() {
        assert_eq!(classify("find my ip address"), LineKind::NaturalLanguage);
        assert_eq!(classify("find large files in home directory"), LineKind::NaturalLanguage);
    }

    #[test]
    fn what_english_sentence_is_nl() {
        assert_eq!(classify("what is my username"), LineKind::NaturalLanguage);
        assert_eq!(classify("what is my ip address"), LineKind::NaturalLanguage);
    }
}
