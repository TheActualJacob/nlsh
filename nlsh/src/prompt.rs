use std::path::PathBuf;

pub struct ShellContext {
    pub cwd: PathBuf,
    pub user: String,
}

impl ShellContext {
    pub fn current() -> Self {
        ShellContext {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            user: std::env::var("USER")
                .or_else(|_| std::env::var("LOGNAME"))
                .unwrap_or_else(|_| "user".into()),
        }
    }
}

pub fn build_prompt(request: &str, ctx: &ShellContext) -> String {
    format!(
        "You are a shell command translator. The user is in a zsh session on macOS.\n\
         Current directory: {cwd}\n\
         User: {user}\n\n\
         Rules:\n\
         - Respond with ONLY a single shell command. No explanation. No markdown. No alternatives.\n\
         - Do not wrap the command in backticks or code fences.\n\
         - Use macOS-compatible commands (e.g. ifconfig not ip, stat -f not stat -c).\n\
         - If the request is ambiguous, prefer the safest interpretation.\n\
         - If the request cannot be translated to a single shell command, output exactly: CANNOT_TRANSLATE\n\n\
         User request: {request}",
        cwd = ctx.cwd.display(),
        user = ctx.user,
        request = request,
    )
}
