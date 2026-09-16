//! Running a helper command and getting its output, or a named failure.

use super::ProbeError;
use std::path::Path;
use std::process::Command;

/// Runs `program` with `args` in `directory` and returns trimmed stdout.
///
/// A non-zero exit code, unreadable output or a missing binary are all errors:
/// the probe never falls back to a guess.
pub fn output_in(directory: &Path, program: &str, args: &[&str]) -> Result<String, ProbeError> {
    let output = Command::new(program)
        .args(args)
        .current_dir(directory)
        .output()
        .map_err(|error| ProbeError::command(program, error.to_string()))?;

    if !output.status.success() {
        let code = output
            .status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string());
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ProbeError::command(
            program,
            format!("exit code {code}: {}", stderr.trim()),
        ));
    }

    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|error| ProbeError::command(program, error.to_string()))
}

/// Runs `program` with `args` in the current directory.
pub fn output(program: &str, args: &[&str]) -> Result<String, ProbeError> {
    output_in(Path::new("."), program, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_command_yields_trimmed_stdout() {
        let text = output("echo", &["  hello  "]).expect("echo runs");
        assert_eq!(text, "hello");
    }

    #[test]
    fn a_missing_binary_is_a_named_failure() {
        let error =
            output("mesh-bench-no-such-binary", &[]).expect_err("missing binaries are errors");
        assert!(matches!(error, ProbeError::Command { .. }));
        assert!(error.to_string().contains("mesh-bench-no-such-binary"));
    }

    #[test]
    fn a_failing_command_is_never_treated_as_empty_output() {
        let error = output("false", &[]).expect_err("non-zero exit is an error");
        assert!(error.to_string().contains("exit code 1"));
    }
}
