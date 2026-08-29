//! Command execution for approved tasks.
//!
//! A command arrives as a single human-readable string (e.g. `rm -rf /tmp/x`).
//! It is parsed shell-style with `shlex` into a program plus an argument vector
//! and executed directly via [`tokio::process::Command`] — never through a
//! shell. Shell metacharacters (`;`, `&&`, `>`, backticks, `$(...)`, ...)
//! therefore become plain arguments and cannot escalate into injection: there is
//! no shell between the operator's approval and the spawned process.

use std::io;
use tokio::process::Command;

/// Parses a command string into the program and its arguments, shell-style.
///
/// * An unbalanced-quote string (for which `shlex` returns `None`) becomes an
///   `InvalidInput` error.
/// * An empty or whitespace-only string becomes an `InvalidInput` error.
///
/// The result is fed straight into [`tokio::process::Command`], which spawns the
/// binary without invoking a shell.
fn parse_command(command_str: &str) -> io::Result<Vec<String>> {
    let tokens = shlex::split(command_str).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "command has unbalanced quotes")
    })?;
    if tokens.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "command is empty",
        ));
    }
    Ok(tokens)
}

/// Executes an approved task's command string and waits for it to complete.
///
/// The command is parsed into a program + argument vector and spawned directly
/// (never through a shell), so the approval applies to the exact binary and
/// arguments rather than to a shell string. Returns the captured
/// [`std::process::Output`].
pub async fn execute_approved_task(command_str: &str) -> io::Result<std::process::Output> {
    let tokens = parse_command(command_str)?;
    Command::new(&tokens[0]).args(&tokens[1..]).output().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_program_from_arguments() {
        let tokens = parse_command("rm -rf /tmp/scratch dir with spaces").unwrap();
        assert_eq!(
            tokens,
            ["rm", "-rf", "/tmp/scratch", "dir", "with", "spaces"]
        );
        assert_eq!(tokens[0], "rm");
    }

    #[test]
    fn keeps_quoted_words_as_single_arguments() {
        let tokens = parse_command("echo \"hello world\" a").unwrap();
        assert_eq!(tokens, ["echo", "hello world", "a"]);
    }

    #[test]
    fn rejects_empty_command() {
        assert_eq!(
            parse_command("   ").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn rejects_unbalanced_quotes() {
        assert_eq!(
            parse_command("echo \"unterminated").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[tokio::test]
    async fn executes_simple_command_and_captures_output() {
        let output = execute_approved_task("echo hello world")
            .await
            .expect("should run");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello world\n");
    }

    #[tokio::test]
    async fn does_not_interpret_shell_metacharacters() {
        // `;` must be a plain argument, never a command separator.
        let output = execute_approved_task("echo hi; ls /")
            .await
            .expect("should run");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hi; ls /\n");

        // `>` must be a plain argument; it must never redirect into a file.
        let output = execute_approved_task("echo x > /dev/null")
            .await
            .expect("should run");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "x > /dev/null\n");
    }
}
