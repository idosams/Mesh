//! The registry: which validators exist, what each one runs, and what fires it.
//!
//! A registry is an **ordered map keyed by [`ValidatorId`]**, not a list. That is the whole of the
//! determinism argument at this layer: a selector walking a `BTreeMap` visits identifiers in the
//! same order on every machine, so two runs over the same registry and the same change produce the
//! same plan in the same order. Registration refuses a duplicate identifier rather than replacing,
//! because a replacement makes the surviving entry depend on registration order.
//!
//! [`ValidatorRegistry::standard`] is the built-in set: one validator per tool plan §9.4 names,
//! each triggered by that tool being detected. It is a starting point a caller may extend or
//! discard, and nothing in this crate treats it as privileged — a standard validator's command is
//! approved by the same profile flow as anybody else's, and until it is approved it does not run.

use std::collections::BTreeMap;

use crate::command::{CommandError, ValidationCommand};
use crate::tooling::DetectedTool;
use crate::trigger::ValidationTrigger;

/// The longest validator identifier, in bytes.
pub const MAX_VALIDATOR_ID_BYTES: usize = 64;

/// A validator's name: lowercase ASCII letters, digits and single dashes.
///
/// Constrained because it is a key, a wire name and something a person reads in an approval
/// prompt. A name that can hold arbitrary text is a name that can be made to look like another
/// one, and an approval prompt is exactly where that matters.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValidatorId(String);

impl ValidatorId {
    /// Parse an identifier.
    ///
    /// # Errors
    ///
    /// [`RegistryError::IllegalValidatorId`] when it is empty, over
    /// [`MAX_VALIDATOR_ID_BYTES`], holds a byte outside `[a-z0-9-]`, begins or ends with a dash,
    /// or holds two dashes in a row.
    pub fn parse(name: &str) -> Result<Self, RegistryError> {
        let legal = !name.is_empty()
            && name.len() <= MAX_VALIDATOR_ID_BYTES
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.contains("--");
        if legal {
            Ok(Self(name.to_owned()))
        } else {
            Err(RegistryError::IllegalValidatorId(name.to_owned()))
        }
    }

    /// The identifier's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for ValidatorId {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Whether a validator's command can be confined to the isolated workspace.
///
/// The distinction is the task's failure-and-recovery clause: a command that cannot be sandboxed
/// is **never run automatically**. [`SandboxRequirement::HostAccess`] is therefore not a weaker
/// sandbox, it is a refusal to run without a separate decision, and the reason travels with it so
/// the user is told what the command wanted rather than that something was skipped.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SandboxRequirement {
    /// The command runs entirely inside the isolated workspace.
    Isolated,
    /// The command needs something outside it — a network, a device, a shared cache.
    HostAccess {
        /// What it needs, in words the user is shown.
        why: String,
    },
}

impl SandboxRequirement {
    /// Whether this command may be started without asking the user again.
    #[must_use]
    pub const fn is_isolated(&self) -> bool {
        matches!(self, Self::Isolated)
    }

    /// The requirement's stable wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Isolated => "isolated",
            Self::HostAccess { .. } => "host-access",
        }
    }
}

/// One validator: what it is called, what it runs, what fires it, and how it must be confined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatorSpec {
    id: ValidatorId,
    command: ValidationCommand,
    triggers: Vec<ValidationTrigger>,
    sandbox: SandboxRequirement,
}

impl ValidatorSpec {
    /// A validator fired by `triggers`, running `command`, confined by `sandbox`.
    ///
    /// # Errors
    ///
    /// [`RegistryError::NoTriggers`] when the trigger list is empty. A validator nothing fires is
    /// either dead or an unconditional one somebody forgot to mark
    /// [`ValidationTrigger::Always`], and both are worth an error rather than a silent never-run.
    pub fn new(
        id: ValidatorId,
        command: ValidationCommand,
        triggers: impl IntoIterator<Item = ValidationTrigger>,
        sandbox: SandboxRequirement,
    ) -> Result<Self, RegistryError> {
        let triggers: Vec<ValidationTrigger> = triggers.into_iter().collect();
        if triggers.is_empty() {
            return Err(RegistryError::NoTriggers(id));
        }
        Ok(Self {
            id,
            command,
            triggers,
            sandbox,
        })
    }

    /// The validator's identifier.
    #[must_use]
    pub const fn id(&self) -> &ValidatorId {
        &self.id
    }

    /// What it runs.
    #[must_use]
    pub const fn command(&self) -> &ValidationCommand {
        &self.command
    }

    /// What fires it, in declared order.
    #[must_use]
    pub fn triggers(&self) -> &[ValidationTrigger] {
        &self.triggers
    }

    /// How it must be confined.
    #[must_use]
    pub const fn sandbox(&self) -> &SandboxRequirement {
        &self.sandbox
    }
}

/// Every validator that exists, keyed by identifier.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidatorRegistry {
    specs: BTreeMap<ValidatorId, ValidatorSpec>,
}

impl ValidatorRegistry {
    /// A registry with nothing in it. A change validated against it plans nothing, which is the
    /// honest answer rather than a default that runs something the user never saw.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// The same registry with one more validator.
    ///
    /// # Errors
    ///
    /// [`RegistryError::DuplicateValidatorId`] when the identifier is already registered.
    pub fn with(mut self, spec: ValidatorSpec) -> Result<Self, RegistryError> {
        if self.specs.contains_key(spec.id()) {
            return Err(RegistryError::DuplicateValidatorId(spec.id().clone()));
        }
        self.specs.insert(spec.id().clone(), spec);
        Ok(self)
    }

    /// The validator registered under `id`.
    #[must_use]
    pub fn get(&self, id: &ValidatorId) -> Option<&ValidatorSpec> {
        self.specs.get(id)
    }

    /// Every validator, in identifier order.
    pub fn iter(&self) -> impl Iterator<Item = &ValidatorSpec> {
        self.specs.values()
    }

    /// How many validators are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Whether nothing is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// The built-in validators: one per tool plan §9.4 names.
    ///
    /// Each is fired by its tool being detected, and each is [`SandboxRequirement::Isolated`]
    /// **as a claim about the command's intent, not a guarantee about the executor** — the
    /// confinement is the executor's to provide, and this crate's part is to refuse to hand over a
    /// command whose specification says it cannot be confined.
    ///
    /// # Panics
    ///
    /// Never in practice: the identifiers and commands here are constants that satisfy their own
    /// constructors, and the tests below exercise every one. The `expect` calls state that.
    #[must_use]
    pub fn standard() -> Self {
        let builtin: [(&str, &str, &[&str], DetectedTool); 6] = [
            (
                "cargo-test",
                "cargo",
                &["nextest", "run"],
                DetectedTool::Cargo,
            ),
            ("npm-test", "npm", &["test"], DetectedTool::Npm),
            ("pnpm-test", "pnpm", &["test"], DetectedTool::Pnpm),
            ("yarn-test", "yarn", &["test"], DetectedTool::Yarn),
            (
                "pytest",
                "python3",
                &["-m", "pytest"],
                DetectedTool::Pyproject,
            ),
            ("go-test", "go", &["test", "./..."], DetectedTool::Go),
        ];

        let mut registry = Self::empty();
        for (id, program, args, tool) in builtin {
            let spec = ValidatorSpec::new(
                ValidatorId::parse(id).expect("a constant identifier"),
                ValidationCommand::at_root(program, args.iter().map(|arg| (*arg).to_owned()))
                    .expect("a constant command"),
                [ValidationTrigger::Tool(tool)],
                SandboxRequirement::Isolated,
            )
            .expect("a constant trigger list");
            registry = registry.with(spec).expect("distinct constant identifiers");
        }

        // `make test` is the one built-in that is NOT auto-runnable. A make file is a program, and
        // what its `test` rule does is unknowable from the path that revealed it, so the
        // requirement records that and the planner refuses to start it without a further decision.
        let make = ValidatorSpec::new(
            ValidatorId::parse("make-test").expect("a constant identifier"),
            ValidationCommand::at_root("make", ["test".to_owned()]).expect("a constant command"),
            [ValidationTrigger::Tool(DetectedTool::Make)],
            SandboxRequirement::HostAccess {
                why: "a make rule is an arbitrary program and what `test` does cannot be read off \
                      the make file's presence"
                    .to_owned(),
            },
        )
        .expect("a constant trigger list");
        registry.with(make).expect("a distinct constant identifier")
    }
}

/// Why a registration was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// The identifier is not a legal [`ValidatorId`].
    IllegalValidatorId(String),
    /// The identifier is already registered.
    DuplicateValidatorId(ValidatorId),
    /// The validator declared no triggers, so nothing would ever fire it.
    NoTriggers(ValidatorId),
    /// A command in a registration could not be built.
    Command(CommandError),
}

impl From<CommandError> for RegistryError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

impl core::fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IllegalValidatorId(name) => {
                write!(formatter, "`{name}` is not a legal validator identifier")
            }
            Self::DuplicateValidatorId(id) => write!(formatter, "`{id}` is already registered"),
            Self::NoTriggers(id) => write!(formatter, "`{id}` declares no triggers"),
            Self::Command(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RegistryError {}

#[cfg(test)]
mod tests {
    use super::{
        RegistryError, SandboxRequirement, ValidatorId, ValidatorRegistry, ValidatorSpec,
        MAX_VALIDATOR_ID_BYTES,
    };
    use crate::command::ValidationCommand;
    use crate::tooling::DetectedTool;
    use crate::trigger::ValidationTrigger;

    fn spec(name: &str) -> ValidatorSpec {
        ValidatorSpec::new(
            ValidatorId::parse(name).expect("a legal identifier"),
            ValidationCommand::at_root("true", []).expect("a legal command"),
            [ValidationTrigger::Always],
            SandboxRequirement::Isolated,
        )
        .expect("one trigger")
    }

    #[test]
    fn an_identifier_is_constrained() {
        for legal in ["a", "cargo-test", "go-test-2", "x9"] {
            assert!(ValidatorId::parse(legal).is_ok(), "`{legal}` was refused");
        }
        for illegal in [
            "",
            "-a",
            "a-",
            "a--b",
            "Cargo",
            "cargo test",
            "cargo_test",
            "cargo/test",
            "cargo\u{0301}",
        ] {
            assert!(
                matches!(
                    ValidatorId::parse(illegal),
                    Err(RegistryError::IllegalValidatorId(_))
                ),
                "`{illegal}` was accepted"
            );
        }
        let long = "a".repeat(MAX_VALIDATOR_ID_BYTES + 1);
        assert!(ValidatorId::parse(&long).is_err());
    }

    #[test]
    fn a_validator_with_no_triggers_is_refused() {
        let error = ValidatorSpec::new(
            ValidatorId::parse("dead").expect("legal"),
            ValidationCommand::at_root("true", []).expect("legal"),
            [],
            SandboxRequirement::Isolated,
        );
        assert!(matches!(error, Err(RegistryError::NoTriggers(_))));
    }

    #[test]
    fn a_duplicate_identifier_is_refused_rather_than_replacing() {
        let registry = ValidatorRegistry::empty()
            .with(spec("one"))
            .expect("the first registration");
        assert!(matches!(
            registry.with(spec("one")),
            Err(RegistryError::DuplicateValidatorId(_))
        ));
    }

    #[test]
    fn iteration_is_in_identifier_order_however_registration_arrived() {
        let forwards = ValidatorRegistry::empty()
            .with(spec("a"))
            .expect("first")
            .with(spec("b"))
            .expect("second");
        let backwards = ValidatorRegistry::empty()
            .with(spec("b"))
            .expect("first")
            .with(spec("a"))
            .expect("second");
        let names = |registry: &ValidatorRegistry| -> Vec<String> {
            registry
                .iter()
                .map(|spec| spec.id().as_str().to_owned())
                .collect()
        };
        assert_eq!(names(&forwards), names(&backwards));
        assert_eq!(names(&forwards), vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(forwards, backwards);
    }

    #[test]
    fn the_standard_registry_covers_every_tool_that_has_a_test_command() {
        let registry = ValidatorRegistry::standard();
        assert_eq!(registry.len(), 7);
        let covered: Vec<DetectedTool> = registry
            .iter()
            .flat_map(|spec| spec.triggers())
            .filter_map(|trigger| match trigger {
                ValidationTrigger::Tool(tool) => Some(*tool),
                _ => None,
            })
            .collect();
        for tool in DetectedTool::ALL {
            if tool == DetectedTool::ContinuousIntegration {
                // CI configuration says a project HAS a pipeline; it does not name a command this
                // crate can propose, so no built-in validator claims it.
                assert!(!covered.contains(&tool));
                continue;
            }
            assert!(covered.contains(&tool), "{tool} has no standard validator");
        }
    }

    #[test]
    fn the_standard_make_validator_is_not_auto_runnable() {
        let registry = ValidatorRegistry::standard();
        let make = registry
            .get(&ValidatorId::parse("make-test").expect("legal"))
            .expect("registered");
        assert!(!make.sandbox().is_isolated());
        assert_eq!(make.sandbox().as_str(), "host-access");
        let others = registry
            .iter()
            .filter(|spec| spec.id().as_str() != "make-test")
            .all(|spec| spec.sandbox().is_isolated());
        assert!(others);
    }

    #[test]
    fn an_empty_registry_holds_nothing() {
        let registry = ValidatorRegistry::empty();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert_eq!(registry.iter().count(), 0);
    }
}
