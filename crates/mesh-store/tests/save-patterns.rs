//! Runtime consumption of every registered save-pattern stream.

#[path = "support/mod.rs"]
mod support;

use std::time::Duration;

use mesh_store::{
    BoundaryEvidenceKind, CheckpointCoordinator, CheckpointRuntimeParameters, RecordDigest,
    RecoveryBoundaryEvidence, RecoveryEventUlid, RecoverySequence, RecoveryStamp,
    RecoveryTransition, RecoveryTrigger,
};
use support::checkpoint_runtime::{private_saved, MemoryPersistence};

const CATALOGUE: &str =
    include_str!("../../../tests/compatibility/save-patterns/v0/catalogue.json");

macro_rules! patterns {
    ($($name:literal),+ $(,)?) => {
        [$(($name, include_str!(concat!(
            "../../../tests/compatibility/save-patterns/v0/patterns/", $name, ".json"
        )))),+]
    };
}

const PATTERNS: [(&str, &str); 16] = patterns![
    "vscode-macos-in-place-truncate",
    "vscode-linux-in-place-truncate",
    "vscode-atomic-replace",
    "jetbrains-safe-write",
    "jetbrains-macos-command-line-format-backup-in-place-truncate",
    "jetbrains-linux-backup-in-place-truncate",
    "vim-in-place-truncate",
    "rustfmt-in-place-truncate",
    "sed-rename-over",
    "sed-rename-over-with-backup",
    "git-checkout-switch-branch",
    "npm-install-cold",
    "linux-vim-rename-over-with-backup",
    "linux-git-checkout-in-place",
    "linux-rustfmt-in-place-truncate",
    "linux-npm-install-cold",
];

fn sequence(value: u64) -> RecoverySequence {
    RecoverySequence::new(value).expect("corpus sequences are non-zero")
}

fn parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_secs(2)),
        maximum_uncheckpointed_bytes: Some(u64::MAX),
        maximum_uncheckpointed_interval: Some(Duration::MAX),
    }
}

fn stream<'a>(document: &'a str, name: &str) -> &'a str {
    let marker = format!("\"{name}\": {{");
    let start = document.find(&marker).expect("stream exists");
    let rest = &document[start..];
    let end_marker = if name == "mount" {
        "\n    \"folder\": {"
    } else {
        "\n  },\n  \"expected\": {"
    };
    &rest[..rest.find(end_marker).expect("stream closes")]
}

fn numbers_after(source: &str, marker: &str) -> Vec<u64> {
    let mut rest = source;
    let mut values = Vec::new();
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let digits = rest
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        values.push(digits.parse().expect("numeric corpus value"));
    }
    values
}

fn candidates(source: &str) -> Vec<(u64, BoundaryEvidenceKind, RecoveryTrigger)> {
    let Some(mut rest) = source.split_once("\"candidates\":").map(|(_, rest)| rest) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    while let Some(at) = rest.find("\"through\": ") {
        rest = &rest[at + 11..];
        let digits = rest
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let through = digits.parse().expect("candidate sequence");
        let reason = rest
            .split_once("\"reason\": \"")
            .map(|(_, tail)| tail.split_once('"').expect("reason closes").0)
            .expect("candidate reason");
        let (kind, trigger) = match reason {
            "Closed" => (
                BoundaryEvidenceKind::Closed,
                RecoveryTrigger::ModifiedFileHandleClosed,
            ),
            "Synced" => (
                BoundaryEvidenceKind::Synced,
                RecoveryTrigger::FsyncCompleted,
            ),
            "RenamedIntoPlace" => (
                BoundaryEvidenceKind::RenamedIntoPlace,
                RecoveryTrigger::AtomicReplacementCompleted,
            ),
            other => panic!("unknown candidate {other}"),
        };
        found.push((through, kind, trigger));
        rest = &rest[digits.len()..];
    }
    found
}

#[test]
fn every_registered_stream_closes_once_only_after_its_final_event() {
    assert_eq!(CATALOGUE.matches("\"observed\": true").count(), 12);
    for (name, document) in PATTERNS {
        assert!(CATALOGUE.contains(&format!("\"pattern\": \"{name}\"")));
        assert!(document.contains(&format!("\"pattern\": \"{name}\"")));
        for stream_name in ["mount", "folder"] {
            let stream = stream(document, stream_name);
            let sequences = numbers_after(stream, "\"sequence\": ");
            let last = *sequences.last().expect("stream has events");
            let mut coordinator =
                CheckpointCoordinator::open(MemoryPersistence::default(), parameters()).unwrap();
            for current in sequences {
                coordinator.observe(sequence(current), 1).unwrap();
                assert!(coordinator.settled_window(Duration::from_secs(1)).is_none());
            }
            for (through, kind, trigger) in candidates(stream) {
                coordinator
                    .record_boundary(
                        trigger,
                        RecoveryBoundaryEvidence::new(sequence(through), kind),
                    )
                    .unwrap();
            }
            let stamp = RecoveryStamp::new(
                last,
                RecoveryEventUlid::from_bytes([last as u8; 16]),
                RecordDigest::from_bytes([last as u8; 32]),
            );
            let settled = coordinator
                .settled_window(Duration::from_secs(2))
                .expect("final inactivity closes the window");
            let transition = coordinator
                .save_settled(settled, stamp, private_saved())
                .unwrap();
            let RecoveryTransition::MeaningfulSaved { checkpoint, .. } = transition else {
                panic!("{name}/{stream_name} did not emit one meaningful result");
            };
            assert_eq!(checkpoint.through(), sequence(last), "{name}/{stream_name}");
            assert!(coordinator.machine().snapshot().open_window().is_none());
        }
    }
}
