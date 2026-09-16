//! Print the validation Mesh would propose for a change, and the approval it would ask for.
//!
//! ```text
//! cargo run -p mesh-validator --example propose-validation -- Cargo.toml src/lib.rs package.json
//! ```
//!
//! This is the user-facing half of plan §9.1 made visible: detection **proposes**, and nothing runs
//! until a human approves the proposal once. Running it shows the exact prompt — which validators a
//! change selects, why each one was selected, and the precise commands the approval would cover.
//!
//! # What it deliberately cannot do
//!
//! It never runs a validator, and that is structural rather than a decision taken here.
//! `mesh-validator` compiles against no filesystem and no process table — `crate::no_ambient_io`
//! asserts it — so nothing reachable from this program can start a process or read a file. Two
//! consequences show up in the output and are printed rather than hidden:
//!
//! * Paths are read from the argument vector and never opened, so each one's size is unknown and
//!   reported as zero. Validators triggered by change volume therefore cannot fire here.
//! * The review's identity is derived from the path list, standing in for the digest a real
//!   immutable review snapshot would carry.
//!
//! This file uses `std::env::args` to read its own argument vector and nothing else ambient. Like
//! `tests/`, an example is a separate binary and gives the library no capability it did not have.

use mesh_types::{Blake3, ContentDigest, Digest32, DigestWriter, DomainTag};
use mesh_validator::{
    plan_validation, ChangedPath, PathEdit, ProfileProposal, ReviewChange, SandboxRequirement,
    ToolingInventory, ValidationPlan, ValidatorRegistry,
};

/// The domain the stand-in review identity is derived in. Distinct from every real one, so a value
/// produced here can never be mistaken for a review snapshot something actually measured.
const DEMO_REVIEW: DomainTag = DomainTag::new("mesh.v0.validator.example.review");

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        print_usage();
        return;
    }

    let borrowed: Vec<&str> = paths.iter().map(String::as_str).collect();
    let tooling = ToolingInventory::detect(borrowed.iter().copied());
    let change = describe(&paths, stand_in_review(&paths));
    let plan = plan_validation(&ValidatorRegistry::standard(), &change, &tooling);

    print_change(&change, &tooling);
    print_plan(&plan);
    print_approval(&plan);
    print_epilogue();
}

/// The change under review, built from the path list alone.
///
/// Every path is recorded as modified at zero bytes: this program never opens a file, so it has no
/// honest size to report and does not invent one.
fn describe(paths: &[String], review: Digest32) -> ReviewChange {
    paths.iter().fold(
        ReviewChange::against(review),
        |change, path| match ChangedPath::new(path, PathEdit::Modified, 0) {
            Ok(changed) => change.clone().with_path(changed).unwrap_or(change),
            Err(_) => change,
        },
    )
}

/// A stable identity for this demonstration, derived from the path list.
fn stand_in_review(paths: &[String]) -> Digest32 {
    let mut writer = DigestWriter::new(DEMO_REVIEW, Blake3::hasher());
    writer.sequence(paths, |writer, path| {
        writer.text(path);
    });
    writer.finish()
}

fn print_usage() {
    println!("Print the validation Mesh would propose for a change, and the approval it asks for.");
    println!();
    println!("  cargo run -p mesh-validator --example propose-validation -- <file> [<file>...]");
    println!();
    println!("Example:");
    println!(
        "  cargo run -p mesh-validator --example propose-validation -- \
         Cargo.toml src/lib.rs package.json"
    );
}

fn print_change(change: &ReviewChange, tooling: &ToolingInventory) {
    println!("The change under review");
    println!("  identity  {}", short(change.snapshot()));
    println!("  files     {}", change.paths().len());
    for path in change.paths() {
        println!("            {}", path.path());
    }
    println!();

    println!("Project tooling detected from those paths alone");
    if tooling.is_empty() {
        println!("  none — no build or test tooling is named by this file list");
    } else {
        for tool in tooling.iter() {
            println!("  {tool}");
        }
    }
    println!();
}

fn print_plan(plan: &ValidationPlan) {
    let automatic = plan
        .steps()
        .iter()
        .filter(|step| matches!(step.sandbox(), SandboxRequirement::Isolated));
    let automatic: Vec<_> = automatic.collect();

    if automatic.is_empty() {
        println!("Mesh proposes no checks for this change.");
    } else {
        println!("Mesh proposes {} check(s) for this change", automatic.len());
    }
    for step in &automatic {
        println!("  {}", step.validator());
        println!("      run   {}", step.command().to_line());
        for reason in step.reasons() {
            println!("      why   {reason}");
        }
    }
    println!();

    let manual: Vec<_> = plan.needing_host_access().collect();
    if !manual.is_empty() {
        println!("Never started automatically — these need your decision each time");
        for step in manual {
            println!("  {}", step.validator());
            println!("      run   {}", step.command().to_line());
            if let SandboxRequirement::HostAccess { why } = step.sandbox() {
                println!("      why not   {why}");
            }
        }
        println!();
    }
}

fn print_approval(plan: &ValidationPlan) {
    let proposal = ProfileProposal::from_plan(plan);
    if proposal.is_empty() {
        println!("There is nothing to approve, so nothing would run.");
        println!();
        return;
    }

    // A command belonging to a step that cannot be confined is in the proposal and still will not
    // run: approval is one of five conditions, and being confinable is another. Saying so is the
    // difference between an honest prompt and one that trains people to wave through host access.
    let unconfinable: Vec<String> = plan
        .needing_host_access()
        .map(|step| step.command().to_line())
        .collect();

    println!("You would be asked once, and only once, to approve exactly this");
    for command in proposal.commands() {
        let line = command.to_line();
        if unconfinable.contains(&line) {
            println!("  {line}   (still never started automatically — see above)");
        } else {
            println!("  {line}");
        }
    }
    println!("  proposal {}", short(proposal.digest()));
    println!();
    println!("  Approving covers these commands and no others: a command differing by one");
    println!("  argument is a different command and would be asked about separately. The");
    println!("  approval stops applying if your workspace policy is rotated.");
    println!();
}

fn print_epilogue() {
    println!("Nothing was run.");
    println!();
    println!("  This program cannot run anything. `mesh-validator` compiles against no");
    println!("  filesystem and no process table, so a validator cannot open, write or start");
    println!("  anything — which is how a validator is kept from changing what it is judging.");
    println!("  Sizes above read as zero for the same reason: no file was opened.");
}

/// The first eight hexadecimal characters of a digest, which is what a person reads.
fn short(digest: Digest32) -> String {
    digest.to_hex().chars().take(8).collect()
}
