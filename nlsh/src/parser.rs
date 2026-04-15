/// Clean raw LLM output down to a single shell command string.
/// Returns None if the output cannot be reduced to a usable command.
pub fn clean_llm_output(raw: &str) -> Option<String> {
    let text = raw.trim();

    if text.is_empty() {
        return None;
    }

    // Strip outer markdown code fence (```lang\n...\n``` or ```\n...\n```).
    let text: &str = if text.starts_with("```") {
        let after_ticks = text.trim_start_matches('`');
        // Skip optional language identifier on the first line.
        let content = match after_ticks.find('\n') {
            Some(nl) => &after_ticks[nl + 1..],
            None => after_ticks,
        };
        // Strip closing fence.
        match content.rfind("```") {
            Some(idx) => content[..idx].trim(),
            None => content.trim(),
        }
    } else {
        text
    };

    // Strip single-line inline backtick wrapping: `command`.
    let text: &str = if !text.contains('\n')
        && text.starts_with('`')
        && text.ends_with('`')
        && text.len() > 2
    {
        &text[1..text.len() - 1]
    } else {
        text
    };

    // From the remaining lines, pick the first that looks like a shell command.
    // Heuristic: a command line starts with a lowercase letter, '$', '/', '.', '-',
    // or contains pipe '|', redirect '>', or '<'.
    let command_line = text
        .lines()
        .map(str::trim)
        .find(|line| {
            if line.is_empty() {
                return false;
            }
            let first = line.chars().next().unwrap();
            first.is_lowercase()
                || matches!(first, '$' | '/' | '.' | '-' | '_')
                || line.contains('|')
                || line.contains('>')
                || line.contains('<')
        })
        .unwrap_or_else(|| text.lines().next().unwrap_or("").trim());

    let result = command_line.trim().to_string();

    if result.is_empty() || result == "CANNOT_TRANSLATE" {
        None
    } else {
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_command() {
        assert_eq!(
            clean_llm_output("df -h"),
            Some("df -h".to_string())
        );
    }

    #[test]
    fn strips_bash_fence() {
        assert_eq!(
            clean_llm_output("```bash\ndf -h\n```"),
            Some("df -h".to_string())
        );
    }

    #[test]
    fn strips_plain_fence() {
        assert_eq!(
            clean_llm_output("```\nls -la\n```"),
            Some("ls -la".to_string())
        );
    }

    #[test]
    fn strips_inline_backtick() {
        assert_eq!(
            clean_llm_output("`ls -la`"),
            Some("ls -la".to_string())
        );
    }

    #[test]
    fn skips_explanation_line() {
        assert_eq!(
            clean_llm_output("Here is the command to show disk usage:\ndf -h"),
            Some("df -h".to_string())
        );
    }

    #[test]
    fn cannot_translate_returns_none() {
        assert_eq!(clean_llm_output("CANNOT_TRANSLATE"), None);
    }

    #[test]
    fn empty_returns_none() {
        assert_eq!(clean_llm_output(""), None);
        assert_eq!(clean_llm_output("   "), None);
    }

    #[test]
    fn pipeline() {
        assert_eq!(
            clean_llm_output("find . -type f | sort -k5 -n"),
            Some("find . -type f | sort -k5 -n".to_string())
        );
    }
}
