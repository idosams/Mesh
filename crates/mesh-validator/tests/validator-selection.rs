//! Selection across the detection matrix, and the determinism claim.
//!
//! Named `validator-selection` because the task contract's automated validation runs
//! `cargo nextest run --test validator-selection` by that exact name.
//!
//! Two things are asserted here that the unit tests cannot:
//!
//! * **The matrix.** Every tool plan §9.4 names is driven end to end — a path list in, a plan out
//!   — rather than each detection rule being checked in isolation against a hand-built inventory.
//! * **Determinism against a shuffle.** The same change set is presented in several arrival orders
//!   and the resulting plan digest is compared, so "deterministic for a given change set" is a
//!   measurement rather than a design intention.

mod support;

use mesh_types::Digest32;
use mesh_validator::{
    plan_validation, ChangeOperation, ChangedPath, DetectedTool, PathEdit, ReviewChange,
    SandboxRequirement, ToolingInventory, ValidationCommand, ValidationTrigger, ValidatorId,
    ValidatorRegistry, ValidatorSpec,
};

use support::snapshot;

/// The path list a project of each kind would present.
fn project_paths(tool: DetectedTool) -> Vec<&'static str> {
    match tool {
        DetectedTool::Cargo => vec!["Cargo.toml", "src/lib.rs"],
        DetectedTool::Npm => vec!["package.json", "package-lock.json"],
        DetectedTool::Pnpm => vec!["package.json", "pnpm-lock.yaml"],
        DetectedTool::Yarn => vec!["package.json", "yarn.lock"],
        DetectedTool::Pyproject => vec!["pyproject.toml", "src/app.py"],
        DetectedTool::Go => vec!["go.mod", "main.go"],
        DetectedTool::Make => vec!["Makefile"],
        DetectedTool::ContinuousIntegration => vec![".github/workflows/ci.yml"],
    }
}

/// The built-in validator each tool selects, when it has one.
fn expected_validator(tool: DetectedTool) -> Option<&'static str> {
    match tool {
        DetectedTool::Cargo => Some("cargo-test"),
        DetectedTool::Npm => Some("npm-test"),
        DetectedTool::Pnpm => Some("pnpm-test"),
        DetectedTool::Yarn => Some("yarn-test"),
        DetectedTool::Pyproject => Some("pytest"),
        DetectedTool::Go => Some("go-test"),
        DetectedTool::Make => Some("make-test"),
        // CI configuration says a project HAS a pipeline; it names no command this crate can
        // propose, so it selects nothing. Asserted rather than assumed.
        DetectedTool::ContinuousIntegration => None,
    }
}

#[test]
fn every_detected_tool_selects_the_validator_the_matrix_says_it_should() {
    let registry = ValidatorRegistry::standard();
    for tool in DetectedTool::ALL {
        let paths = project_paths(tool);
        let tooling = ToolingInventory::detect(paths.iter().copied());
        assert!(
            tooling.contains(tool),
            "{tool} was not detected from {paths:?}"
        );

        let plan = plan_validation(&registry, &support::change(), &tooling);
        let selected: Vec<&str> = plan
            .steps()
            .iter()
            .map(|step| step.validator().as_str())
            .collect();

        match expected_validator(tool) {
            Some(expected) => assert_eq!(
                selected,
                vec![expected],
                "{tool} selected {selected:?}, not `{expected}`"
            ),
            None => assert!(selected.is_empty(), "{tool} selected {selected:?}"),
        }
    }
}

#[test]
fn a_project_using_two_tools_selects_both_validators_in_identifier_order() {
    let tooling = ToolingInventory::detect(["Cargo.toml", "go.mod", "package.json"]);
    let plan = plan_validation(&ValidatorRegistry::standard(), &support::change(), &tooling);
    let selected: Vec<&str> = plan
        .steps()
        .iter()
        .map(|step| step.validator().as_str())
        .collect();
    assert_eq!(selected, vec!["cargo-test", "go-test", "npm-test"]);
}

#[test]
fn the_plan_is_identical_however_the_change_was_assembled() {
    let registry = ValidatorRegistry::empty()
        .with(
            ValidatorSpec::new(
                ValidatorId::parse("rust").expect("legal"),
                ValidationCommand::at_root("cargo", ["test".to_owned()]).expect("legal"),
                [
                    ValidationTrigger::PathSuffix(".rs".to_owned()),
                    ValidationTrigger::PathsAtLeast(2),
                    ValidationTrigger::Operation(ChangeOperation::EditContent),
                ],
                SandboxRequirement::Isolated,
            )
            .expect("three triggers"),
        )
        .expect("registered")
        .with(
            ValidatorSpec::new(
                ValidatorId::parse("conflict-review").expect("legal"),
                ValidationCommand::at_root("review", []).expect("legal"),
                [ValidationTrigger::ConflictsPresent],
                SandboxRequirement::Isolated,
            )
            .expect("one trigger"),
        )
        .expect("registered");

    let paths = [
        ("a/one.rs", PathEdit::Modified, 10_u64),
        ("b/two.rs", PathEdit::Added, 20),
        ("c/three.md", PathEdit::Removed, 0),
    ];

    let orders: [[usize; 3]; 4] = [[0, 1, 2], [2, 1, 0], [1, 0, 2], [1, 2, 0]];
    let digests: Vec<Digest32> = orders
        .iter()
        .map(|order| {
            let mut change = ReviewChange::against(snapshot())
                .with_conflicts(1)
                .with_operation(ChangeOperation::EditContent);
            for at in order {
                let (path, edit, bytes) = paths[*at];
                change = change
                    .with_path(ChangedPath::new(path, edit, bytes).expect("a legal path"))
                    .expect("no duplicate");
            }
            plan_validation(&registry, &change, &ToolingInventory::empty()).digest()
        })
        .collect();

    for digest in &digests {
        assert_eq!(
            *digest, digests[0],
            "the plan digest depends on the order the change was assembled in"
        );
    }
}

#[test]
fn planning_twice_over_the_same_inputs_produces_the_same_plan() {
    let registry = ValidatorRegistry::standard();
    let tooling = ToolingInventory::detect(["Cargo.toml", "Makefile", "package.json"]);
    let change = support::change();
    let first = plan_validation(&registry, &change, &tooling);
    let second = plan_validation(&registry, &change, &tooling);
    assert_eq!(first, second);
    assert_eq!(first.digest(), second.digest());
}

#[test]
fn a_change_that_selects_nothing_produces_an_empty_plan_rather_than_a_default() {
    let plan = plan_validation(
        &ValidatorRegistry::standard(),
        &support::change(),
        &ToolingInventory::empty(),
    );
    assert!(
        plan.is_empty(),
        "an undetected project must propose nothing, not a guess"
    );
}

#[test]
fn every_selected_step_records_why_it_was_selected() {
    let plan = plan_validation(
        &ValidatorRegistry::standard(),
        &support::change(),
        &ToolingInventory::detect(["Cargo.toml", "go.mod"]),
    );
    assert_eq!(plan.len(), 2);
    for step in plan.steps() {
        assert!(
            !step.reasons().is_empty(),
            "`{}` is in the plan with no reason",
            step.validator()
        );
        for reason in step.reasons() {
            assert!(!reason.to_string().is_empty());
            assert!(!reason.axis().is_empty());
        }
    }
}

#[test]
fn selection_reads_the_conflict_and_staleness_axes() {
    let registry = ValidatorRegistry::empty()
        .with(
            ValidatorSpec::new(
                ValidatorId::parse("on-conflict").expect("legal"),
                ValidationCommand::at_root("conflict", []).expect("legal"),
                [ValidationTrigger::ConflictsPresent],
                SandboxRequirement::Isolated,
            )
            .expect("one trigger"),
        )
        .expect("registered")
        .with(
            ValidatorSpec::new(
                ValidatorId::parse("on-stale").expect("legal"),
                ValidationCommand::at_root("stale", []).expect("legal"),
                [ValidationTrigger::StaleInputsPresent],
                SandboxRequirement::Isolated,
            )
            .expect("one trigger"),
        )
        .expect("registered");

    let clean = plan_validation(&registry, &support::change(), &ToolingInventory::empty());
    assert!(clean.is_empty());

    let conflicted = plan_validation(
        &registry,
        &support::change().with_conflicts(2),
        &ToolingInventory::empty(),
    );
    assert_eq!(conflicted.len(), 1);
    assert_eq!(conflicted.steps()[0].validator().as_str(), "on-conflict");

    let stale = plan_validation(
        &registry,
        &support::change().with_conflicts(2).with_stale_inputs(1),
        &ToolingInventory::empty(),
    );
    assert_eq!(stale.len(), 2);
}
