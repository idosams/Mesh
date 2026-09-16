//! A validation command, and the two digests a validation record is keyed by.
//!
//! # A command is an argument vector, never a shell string
//!
//! [`ValidationCommand`] holds a program and its arguments separately and offers no way to build
//! one from a single line of text. There is no interpreter between this type and the process a
//! caller eventually spawns, so `;`, `|`, `$(…)`, a newline and a redirection are ordinary bytes
//! in an argument rather than a second command. The constructor refuses them in the *program*
//! anyway, because a program name that needs a shell metacharacter is a shell string somebody
//! flattened, and refusing it here is cheaper than discovering it in an executor.
//!
//! # The digest is the identity a profile approves
//!
//! [`CommandDigest`] frames every field with its length, so no two distinct commands share a
//! digest and no rearrangement of arguments produces the same one. That is what makes a
//! [`ValidationProfile`](crate::ValidationProfile) an approval of *these* commands and not of a
//! name that a later edit could point somewhere else: changing one byte of an argument produces a
//! command the profile does not admit, and an unadmitted command is never handed to an executor.
//!
//! # The environment digest is the caller's observation
//!
//! [`EnvironmentDigest`] is computed from key/value pairs the caller supplies. This crate reads no
//! environment — it cannot, and `no_ambient_io` asserts that at compile time — so the digest is a
//! record of what the caller says the run saw. It is recorded as evidence, and what it is good for
//! is comparison: two runs of the same command with the same digest saw the same environment, and
//! two runs with different digests are not each other's reproduction.

use std::collections::BTreeMap;

use mesh_types::{Absorb, Blake3, ContentDigest, Digest32, DigestHasher, DigestWriter, DomainTag};

/// The longest program name or argument this crate will carry, in bytes.
pub const MAX_ARGUMENT_BYTES: usize = 4096;

/// The most arguments one command may carry.
pub const MAX_ARGUMENTS: usize = 64;

/// The domain a command's digest is derived in.
const COMMAND_DOMAIN: DomainTag = DomainTag::new("mesh.v0.validator.command");

/// The domain an environment's digest is derived in.
const ENVIRONMENT_DOMAIN: DomainTag = DomainTag::new("mesh.v0.validator.environment");

/// Bytes a program name may never hold. Every one of them is a shell operator, and a program name
/// holding one is a flattened shell line.
const SHELL_BYTES: [char; 12] = [
    ';', '|', '&', '$', '>', '<', '`', '\n', '\r', '\0', '(', ')',
];

/// The identity a profile approves: the digest of one exact command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandDigest(Digest32);

impl CommandDigest {
    /// The digest's bytes.
    #[must_use]
    pub const fn digest(self) -> Digest32 {
        self.0
    }

    /// The digest as lowercase hexadecimal, for a record a person reads.
    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }
}

impl From<Digest32> for CommandDigest {
    fn from(digest: Digest32) -> Self {
        Self(digest)
    }
}

/// What a validation run executes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValidationCommand {
    program: String,
    args: Vec<String>,
    workdir: String,
}

impl ValidationCommand {
    /// One command, run from `workdir` relative to the isolated workspace root.
    ///
    /// # Errors
    ///
    /// [`CommandError`] when the program is empty, over-long or holds a shell operator; when an
    /// argument is over-long or holds a NUL; when there are more than [`MAX_ARGUMENTS`] arguments;
    /// or when `workdir` is not a relative path without traversal components.
    pub fn new(
        program: &str,
        args: impl IntoIterator<Item = String>,
        workdir: &str,
    ) -> Result<Self, CommandError> {
        if program.is_empty() {
            return Err(CommandError::EmptyProgram);
        }
        if program.len() > MAX_ARGUMENT_BYTES {
            return Err(CommandError::TooLong {
                bytes: program.len(),
            });
        }
        if program.contains(SHELL_BYTES) {
            return Err(CommandError::ShellOperatorInProgram(program.to_owned()));
        }
        let args: Vec<String> = args.into_iter().collect();
        if args.len() > MAX_ARGUMENTS {
            return Err(CommandError::TooManyArguments { count: args.len() });
        }
        for argument in &args {
            if argument.len() > MAX_ARGUMENT_BYTES {
                return Err(CommandError::TooLong {
                    bytes: argument.len(),
                });
            }
            if argument.contains('\0') {
                return Err(CommandError::NulInArgument(argument.clone()));
            }
        }
        validate_workdir(workdir)?;
        Ok(Self {
            program: program.to_owned(),
            args,
            workdir: workdir.to_owned(),
        })
    }

    /// A command run from the isolated workspace root.
    ///
    /// # Errors
    ///
    /// As [`ValidationCommand::new`].
    pub fn at_root(
        program: &str,
        args: impl IntoIterator<Item = String>,
    ) -> Result<Self, CommandError> {
        Self::new(program, args, "")
    }

    /// The program to execute. Never a shell.
    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The arguments, in order, each one already separate from the others.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// The working directory, relative to the isolated workspace root. Empty means the root.
    #[must_use]
    pub fn workdir(&self) -> &str {
        &self.workdir
    }

    /// This command's identity.
    #[must_use]
    pub fn digest(&self) -> CommandDigest {
        let mut writer = DigestWriter::new(COMMAND_DOMAIN, Blake3::hasher());
        self.absorb(&mut writer);
        CommandDigest(writer.finish())
    }

    /// The command as one line, for a record a person reads.
    ///
    /// **Not** a shell line, and never fed back to a shell: it is a rendering, and the round trip
    /// does not exist because [`ValidationCommand`] cannot be parsed from text.
    #[must_use]
    pub fn to_line(&self) -> String {
        let mut line = String::from(&self.program);
        for argument in &self.args {
            line.push(' ');
            line.push_str(argument);
        }
        line
    }
}

impl Absorb for ValidationCommand {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.text(&self.program);
        writer.sequence(&self.args, |writer, argument| {
            writer.text(argument);
        });
        writer.text(&self.workdir);
    }
}

impl core::fmt::Display for ValidationCommand {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.to_line())
    }
}

/// The digest of the environment a validation run observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnvironmentDigest(Digest32);

impl EnvironmentDigest {
    /// The digest of the key/value pairs a caller observed.
    ///
    /// Ordered by key inside a `BTreeMap`, so the caller's iteration order does not reach the
    /// digest. A repeated key keeps the last value supplied, which is what a process environment
    /// does with a repeated assignment.
    #[must_use]
    pub fn of(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        let sorted: BTreeMap<String, String> = entries.into_iter().collect();
        let pairs: Vec<(String, String)> = sorted.into_iter().collect();
        let mut writer = DigestWriter::new(ENVIRONMENT_DOMAIN, Blake3::hasher());
        writer.sequence(&pairs, |writer, (key, value)| {
            writer.text(key);
            writer.text(value);
        });
        Self(writer.finish())
    }

    /// The digest of an empty environment.
    ///
    /// A real value, not a placeholder: a run in a sandbox that clears the environment has one,
    /// and a step that never ran carries no environment digest at all rather than this one.
    #[must_use]
    pub fn empty() -> Self {
        Self::of([])
    }

    /// The digest's bytes.
    #[must_use]
    pub const fn digest(self) -> Digest32 {
        self.0
    }

    /// The digest as lowercase hexadecimal.
    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }
}

/// Why a command was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandError {
    /// The program name was empty.
    EmptyProgram,
    /// A program name or argument was over [`MAX_ARGUMENT_BYTES`].
    TooLong {
        /// How many bytes it held.
        bytes: usize,
    },
    /// There were more than [`MAX_ARGUMENTS`] arguments.
    TooManyArguments {
        /// How many were supplied.
        count: usize,
    },
    /// The program name held a shell operator, so it is a shell line somebody flattened.
    ShellOperatorInProgram(String),
    /// An argument held a NUL, which no argument vector can carry.
    NulInArgument(String),
    /// The working directory was absolute, or held a traversal component.
    IllegalWorkdir(String),
}

impl core::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyProgram => formatter.write_str("the program name is empty"),
            Self::TooLong { bytes } => write!(formatter, "a command field is {bytes} bytes"),
            Self::TooManyArguments { count } => write!(formatter, "{count} arguments is too many"),
            Self::ShellOperatorInProgram(program) => {
                write!(formatter, "`{program}` holds a shell operator")
            }
            Self::NulInArgument(argument) => write!(formatter, "`{argument}` holds a NUL"),
            Self::IllegalWorkdir(workdir) => {
                write!(formatter, "`{workdir}` is not a relative working directory")
            }
        }
    }
}

impl std::error::Error for CommandError {}

/// The working directory a command may name: relative, without traversal.
fn validate_workdir(workdir: &str) -> Result<(), CommandError> {
    if workdir.is_empty() {
        return Ok(());
    }
    if workdir.len() > MAX_ARGUMENT_BYTES {
        return Err(CommandError::TooLong {
            bytes: workdir.len(),
        });
    }
    let illegal = workdir.starts_with('/')
        || workdir.contains('\0')
        || workdir.contains('\\')
        || workdir
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    if illegal {
        return Err(CommandError::IllegalWorkdir(workdir.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CommandError, EnvironmentDigest, ValidationCommand, MAX_ARGUMENTS, MAX_ARGUMENT_BYTES,
    };

    fn cargo_test() -> ValidationCommand {
        ValidationCommand::at_root("cargo", ["nextest".to_owned(), "run".to_owned()])
            .expect("a legal command")
    }

    #[test]
    fn the_digest_is_stable_and_distinguishes_every_field() {
        let base = cargo_test();
        assert_eq!(base.digest(), cargo_test().digest());

        let other_program =
            ValidationCommand::at_root("cargo2", ["nextest".to_owned(), "run".to_owned()])
                .expect("legal");
        let other_args =
            ValidationCommand::at_root("cargo", ["run".to_owned(), "nextest".to_owned()])
                .expect("legal");
        let other_workdir =
            ValidationCommand::new("cargo", ["nextest".to_owned(), "run".to_owned()], "crates")
                .expect("legal");
        for other in [other_program, other_args, other_workdir] {
            assert_ne!(base.digest(), other.digest(), "`{other}` shares a digest");
        }
    }

    #[test]
    fn argument_boundaries_cannot_be_shifted() {
        let joined =
            ValidationCommand::at_root("cargo", ["nextest run".to_owned()]).expect("legal");
        assert_ne!(cargo_test().digest(), joined.digest());
    }

    #[test]
    fn a_shell_operator_in_the_program_is_refused() {
        for program in [
            "cargo; rm -rf /",
            "cargo | tee",
            "cargo && echo",
            "$(whoami)",
            "cargo\nrm",
            "cargo > out",
        ] {
            assert!(
                matches!(
                    ValidationCommand::at_root(program, []),
                    Err(CommandError::ShellOperatorInProgram(_))
                ),
                "`{program}` was accepted"
            );
        }
    }

    #[test]
    fn an_argument_may_hold_anything_a_shell_would_have_interpreted() {
        let command = ValidationCommand::at_root("grep", ["a;b|c$(d)".to_owned()])
            .expect("an argument is bytes, not a command");
        assert_eq!(command.args(), ["a;b|c$(d)".to_owned()]);
    }

    #[test]
    fn the_boundary_checks_bite() {
        assert_eq!(
            ValidationCommand::at_root("", []),
            Err(CommandError::EmptyProgram)
        );
        let long = "a".repeat(MAX_ARGUMENT_BYTES + 1);
        assert!(matches!(
            ValidationCommand::at_root(&long, []),
            Err(CommandError::TooLong { .. })
        ));
        assert!(matches!(
            ValidationCommand::at_root("cargo", [long.clone()]),
            Err(CommandError::TooLong { .. })
        ));
        assert!(matches!(
            ValidationCommand::at_root("cargo", vec!["a".to_owned(); MAX_ARGUMENTS + 1]),
            Err(CommandError::TooManyArguments { .. })
        ));
        assert!(matches!(
            ValidationCommand::at_root("cargo", ["a\0b".to_owned()]),
            Err(CommandError::NulInArgument(_))
        ));
        for workdir in ["/abs", "a/../b", "./a", "a//b", "a\\b"] {
            assert!(
                matches!(
                    ValidationCommand::new("cargo", [], workdir),
                    Err(CommandError::IllegalWorkdir(_))
                ),
                "`{workdir}` was accepted as a working directory"
            );
        }
        assert!(ValidationCommand::new("cargo", [], "crates/mesh-validator").is_ok());
    }

    #[test]
    fn the_environment_digest_ignores_the_callers_iteration_order() {
        let forwards = EnvironmentDigest::of([
            ("A".to_owned(), "1".to_owned()),
            ("B".to_owned(), "2".to_owned()),
        ]);
        let backwards = EnvironmentDigest::of([
            ("B".to_owned(), "2".to_owned()),
            ("A".to_owned(), "1".to_owned()),
        ]);
        assert_eq!(forwards, backwards);
        assert_ne!(forwards, EnvironmentDigest::empty());
    }

    #[test]
    fn a_changed_environment_value_changes_the_digest() {
        let one = EnvironmentDigest::of([("PATH".to_owned(), "/usr/bin".to_owned())]);
        let two = EnvironmentDigest::of([("PATH".to_owned(), "/usr/local/bin".to_owned())]);
        assert_ne!(one, two);
    }

    #[test]
    fn the_rendering_is_not_a_parser() {
        assert_eq!(cargo_test().to_line(), "cargo nextest run");
        assert_eq!(cargo_test().to_string(), "cargo nextest run");
    }

    #[test]
    fn the_digest_hex_is_sixty_four_characters() {
        assert_eq!(cargo_test().digest().to_hex().len(), 64);
        assert_eq!(EnvironmentDigest::empty().to_hex().len(), 64);
    }
}
