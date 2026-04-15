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
        Ok(status) if status.success() => LineKind::Shell,
        _ => LineKind::NaturalLanguage,
    }
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
}
