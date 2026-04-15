/// Patterns that indicate a destructive or high-risk command.
/// Matched case-insensitively against the full command string.
const DESTRUCTIVE_PATTERNS: &[&str] = &[
    "rm -rf",
    "rm -fr",
    "rm -r",
    "dd if=",
    "mkfs",
    ":(){ :|:",
    "chmod -r 777",
    "chmod 777",
    "> /dev/",
    "shred ",
    "wipefs",
    "fdisk",
    "diskutil erasedisk",
    "diskutil erasevolume",
    "format ",
    "truncate ",
    "mv / ",
    "mv /* ",
];

pub fn is_destructive(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    DESTRUCTIVE_PATTERNS.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rm_rf_is_destructive() {
        assert!(is_destructive("rm -rf /tmp/test"));
    }

    #[test]
    fn ls_is_not_destructive() {
        assert!(!is_destructive("ls -la"));
    }

    #[test]
    fn dd_is_destructive() {
        assert!(is_destructive("dd if=/dev/urandom of=/dev/sda"));
    }
}
