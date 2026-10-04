//! The table set, and the provenance declaration that makes the reconstruction rule mechanical.
//!
//! Every table names the [`RecordKind`]s it is a fold over. A table with no source cannot be
//! declared — [`Provenance`] has exactly two shapes, and the only non-record one is reserved for
//! `schema_version`, which is the migration ledger and not an index over anything.
//!
//! The declarations here and the `CREATE TABLE` statements in [`crate::MIGRATIONS`] are two
//! surfaces that could drift, so they are held together from both sides in
//! `tests/reconstruction.rs`: every table named in the migration SQL appears here, and every table
//! named here appears in the migration SQL. Adding a table to the schema without declaring where
//! it can be rebuilt from turns that test red.

use crate::record::RecordKind;

/// The SQLite storage class a column holds.
///
/// Only two, and that is a decision rather than an omission: no record-derived column holds free
/// text, so [`crate::Statement`] renders literals without ever quoting a string, and the class of
/// SQL-quoting bug that comes with string literals is absent by construction rather than avoided
/// by care. Adding a text column means adding a variant here *and* the escaping it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnType {
    /// A 64-bit signed integer.
    Integer,
    /// A byte string of a fixed width, checked by the table's `CHECK` constraint.
    Blob(usize),
}

impl ColumnType {
    /// The `STRICT`-table keyword for this class.
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Blob(_) => "BLOB",
        }
    }
}

/// What a column *means*, as opposed to what it stores.
///
/// This is where the `mesh-types` identity split survives the narrowing described in
/// [`crate::RecordDigest`]. `mesh_types_item` names the `mesh-types` public item a column mirrors,
/// and `tests/mesh_types_drift.rs` reads `mesh-types`' own source to check that every name here
/// still exists there and that every record identifier there is either mirrored or listed in
/// [`UNINDEXED_MESH_TYPES_IDS`] with a reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnDomain {
    /// A short human label, used in errors and in the schema documentation.
    pub label: &'static str,
    /// The `mesh-types` item this column mirrors, when it mirrors one.
    pub mesh_types_item: Option<&'static str>,
}

impl ColumnDomain {
    /// A domain that mirrors a `mesh-types` public item.
    #[must_use]
    pub const fn mirroring(label: &'static str, item: &'static str) -> Self {
        Self {
            label,
            mesh_types_item: Some(item),
        }
    }

    /// A domain local to the index, with no `mesh-types` counterpart.
    #[must_use]
    pub const fn local(label: &'static str) -> Self {
        Self {
            label,
            mesh_types_item: None,
        }
    }
}

/// One column of one table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    /// The column name, exactly as the migration SQL spells it.
    pub name: &'static str,
    /// What it stores.
    pub column_type: ColumnType,
    /// What it means.
    pub domain: ColumnDomain,
}

/// Where a table's rows can be rebuilt from.
///
/// The whole point of the type is that there is no third shape. A fact that is neither a fold over
/// immutable records nor the migration ledger has nowhere to be declared, which is the task's rule
/// — "if a fact cannot be rebuilt, it does not belong in this database" — expressed as something
/// the compiler holds rather than something a reviewer remembers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// A fold over these record kinds, in the order they contribute.
    Records(&'static [RecordKind]),
    /// The migration ledger itself: written by applying a migration, read by the next one.
    /// Exactly one table may carry this, which `tests/reconstruction.rs` checks.
    MigrationLedger,
}

impl Provenance {
    /// The record kinds this provenance folds, empty for the migration ledger.
    #[must_use]
    pub const fn record_kinds(&self) -> &'static [RecordKind] {
        match self {
            Self::Records(kinds) => kinds,
            Self::MigrationLedger => &[],
        }
    }
}

/// One table: its name, its columns, where it is rebuilt from, and why it exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Table {
    /// The table name, exactly as the migration SQL spells it.
    pub name: &'static str,
    /// The schema version that introduced it.
    pub since_version: u32,
    /// Its columns, in declaration order — the order [`crate::Row`] values are in.
    pub columns: &'static [Column],
    /// Where its rows come from.
    pub provenance: Provenance,
    /// One sentence on what the table is for, quoted into the schema documentation.
    pub purpose: &'static str,
}

impl Table {
    /// The column names, in order.
    #[must_use]
    pub fn column_names(&self) -> Vec<&'static str> {
        self.columns.iter().map(|column| column.name).collect()
    }
}

const OPERATION_ID: ColumnDomain = ColumnDomain::mirroring("operation identifier", "ChangeSetId");
const ACTOR_ID: ColumnDomain = ColumnDomain::mirroring("actor identifier", "ActorId");
const MANIFEST_ID: ColumnDomain = ColumnDomain::mirroring("manifest identifier", "ManifestId");
const CONTENT_HASH: ColumnDomain = ColumnDomain::mirroring("content hash", "ContentHash");
const BUNDLE_ID: ColumnDomain =
    ColumnDomain::mirroring("review bundle identifier", "ReviewBundleId");
const APPROVAL_ID: ColumnDomain = ColumnDomain::mirroring("approval identifier", "ApprovalId");
const SESSION_ID: ColumnDomain = ColumnDomain::mirroring("session identifier", "SessionId");
const ENTRY_ID: ColumnDomain = ColumnDomain::local("context ledger entry identifier");
const SEQUENCE: ColumnDomain = ColumnDomain::local("actor sequence number");
const ORDINAL: ColumnDomain = ColumnDomain::local("position within a record's own list");
const BYTE_COUNT: ColumnDomain = ColumnDomain::local("a count of bytes");
const HLC_PART: ColumnDomain = ColumnDomain::local("hybrid logical time, which orders nothing");
const POLICY_EPOCH: ColumnDomain = ColumnDomain::local("policy epoch");
const ENUM_CODE: ColumnDomain = ColumnDomain::local("an enumeration code");
const LEDGER_FIELD: ColumnDomain = ColumnDomain::local("a migration ledger field");

const DIGEST: ColumnType = ColumnType::Blob(32);
const UUID: ColumnType = ColumnType::Blob(16);
const FINGERPRINT: ColumnType = ColumnType::Blob(16);
const INT: ColumnType = ColumnType::Integer;

const fn column(name: &'static str, column_type: ColumnType, domain: ColumnDomain) -> Column {
    Column {
        name,
        column_type,
        domain,
    }
}

/// `mesh-types` record identifiers this index deliberately does not hold, each with the reason.
///
/// The list exists so that "we do not index it" is a decision on the record rather than an
/// omission nobody noticed. `tests/mesh_types_drift.rs` asserts that the identifiers `mesh-types`
/// declares are exactly the ones mirrored by a column plus the ones listed here — so a *new*
/// record identifier in `mesh-types` turns this crate's tests red until somebody decides which
/// side of the line it falls on.
pub const UNINDEXED_MESH_TYPES_IDS: &[(&str, &str)] = &[
    (
        "VersionId",
        "object versions are the state projection's to hold; plan §6.1's storage list for the \
         local transactional store does not include them",
    ),
    (
        "HeadId",
        "actor_head stores the head operation, not a separate head record; a HeadId names a record \
         the sync protocol exchanges, and it is indexed when that record exists",
    ),
];

/// Every table, in the order the reconstruction digest absorbs them.
///
/// The order is fixed and part of the digest: reordering this list changes every digest, which is
/// why it is appended to rather than sorted.
pub const TABLES: &[Table] = &[
    Table {
        name: "schema_version",
        since_version: 1,
        columns: &[
            column("version", INT, LEDGER_FIELD),
            column("sql_fingerprint", FINGERPRINT, LEDGER_FIELD),
        ],
        provenance: Provenance::MigrationLedger,
        purpose: "The forward-only migration ledger: one row per applied migration, carrying the \
                  fingerprint of the SQL that ran, so a rewritten migration is detected rather \
                  than silently re-applied under a version it no longer matches.",
    },
    Table {
        name: "operation",
        since_version: 1,
        columns: &[
            column("operation_id", DIGEST, OPERATION_ID),
            column("actor_id", DIGEST, ACTOR_ID),
            column("actor_sequence", INT, SEQUENCE),
            column("hlc_millis", INT, HLC_PART),
            column("hlc_counter", INT, HLC_PART),
            column("policy_epoch", INT, POLICY_EPOCH),
            column("session_id", UUID, SESSION_ID),
            column("payload_digest", DIGEST, CONTENT_HASH),
        ],
        provenance: Provenance::Records(&[RecordKind::Operation]),
        purpose: "Plan §6.1's operation index: one immutable row per canonical operation.",
    },
    Table {
        name: "operation_parent",
        since_version: 1,
        columns: &[
            column("operation_id", DIGEST, OPERATION_ID),
            column("ordinal", INT, ORDINAL),
            column("parent_id", DIGEST, OPERATION_ID),
        ],
        provenance: Provenance::Records(&[RecordKind::Operation]),
        purpose: "The causal edges an operation seals, in the author's own order.",
    },
    Table {
        name: "manifest",
        since_version: 1,
        columns: &[
            column("manifest_id", DIGEST, MANIFEST_ID),
            column("byte_length", INT, BYTE_COUNT),
            column("content_digest", DIGEST, CONTENT_HASH),
        ],
        provenance: Provenance::Records(&[RecordKind::Manifest]),
        purpose: "File manifests. The chunk bytes they name live in the content-addressed store.",
    },
    Table {
        name: "manifest_chunk",
        since_version: 1,
        columns: &[
            column("manifest_id", DIGEST, MANIFEST_ID),
            column("ordinal", INT, ORDINAL),
            column("chunk_digest", DIGEST, CONTENT_HASH),
            column("byte_offset", INT, BYTE_COUNT),
            column("byte_length", INT, BYTE_COUNT),
        ],
        provenance: Provenance::Records(&[RecordKind::Manifest]),
        purpose: "A manifest's chunk references, in reconstruction order.",
    },
    Table {
        name: "actor_head",
        since_version: 1,
        columns: &[
            column("actor_id", DIGEST, ACTOR_ID),
            column("operation_id", DIGEST, OPERATION_ID),
            column("actor_sequence", INT, SEQUENCE),
        ],
        provenance: Provenance::Records(&[RecordKind::Operation]),
        purpose: "Each actor's highest causally-ready operation. Missing-parent records remain in \
                  the operation tables until their closure is present and cannot advance this \
                  plan §6.3 step 7 projection.",
    },
    Table {
        name: "peer",
        since_version: 1,
        columns: &[
            column("peer_id", DIGEST, ACTOR_ID),
            column("joined_at_operation", DIGEST, OPERATION_ID),
        ],
        provenance: Provenance::Records(&[RecordKind::Peer]),
        purpose: "The replication set. Needed so the outbox has a reconstructable peer list.",
    },
    Table {
        name: "peer_watermark",
        since_version: 1,
        columns: &[
            column("peer_id", DIGEST, ACTOR_ID),
            column("actor_id", DIGEST, ACTOR_ID),
            column("actor_sequence", INT, SEQUENCE),
        ],
        provenance: Provenance::Records(&[RecordKind::Acknowledgement]),
        purpose: "The highest contiguous sequence each peer has acknowledged, per actor.",
    },
    Table {
        name: "outbox",
        since_version: 1,
        columns: &[
            column("peer_id", DIGEST, ACTOR_ID),
            column("actor_id", DIGEST, ACTOR_ID),
            column("actor_sequence", INT, SEQUENCE),
            column("operation_id", DIGEST, OPERATION_ID),
        ],
        provenance: Provenance::Records(&[
            RecordKind::Operation,
            RecordKind::Peer,
            RecordKind::Acknowledgement,
        ]),
        purpose: "Plan §6.1's durable outbox, held as the difference between what exists and what \
                  each peer has acknowledged rather than as a queue rows are deleted from.",
    },
    Table {
        name: "review_bundle",
        since_version: 2,
        columns: &[
            column("bundle_id", DIGEST, BUNDLE_ID),
            column("subject_operation_id", DIGEST, OPERATION_ID),
            column("opened_by_actor_id", DIGEST, ACTOR_ID),
        ],
        provenance: Provenance::Records(&[RecordKind::Review]),
        purpose: "Review bundles opened over an operation.",
    },
    Table {
        name: "review_approval",
        since_version: 2,
        columns: &[
            column("approval_id", DIGEST, APPROVAL_ID),
            column("bundle_id", DIGEST, BUNDLE_ID),
            column("approver_actor_id", DIGEST, ACTOR_ID),
            column("verdict", INT, ENUM_CODE),
        ],
        provenance: Provenance::Records(&[RecordKind::Approval]),
        purpose: "Approval envelopes. A withdrawal is a later envelope, never an edit.",
    },
    Table {
        name: "context_ledger",
        since_version: 2,
        columns: &[
            column("entry_id", DIGEST, ENTRY_ID),
            column("session_id", UUID, SESSION_ID),
            column("operation_id", DIGEST, OPERATION_ID),
            column("access", INT, ENUM_CODE),
            column("byte_length", INT, BYTE_COUNT),
        ],
        provenance: Provenance::Records(&[RecordKind::ContextEntry]),
        purpose: "Which session touched which operation's content, and how.",
    },
    Table {
        name: "dependency_record",
        since_version: 3,
        columns: &[
            column(
                "authority_id",
                DIGEST,
                ColumnDomain::local("native dependency authority"),
            ),
            column("revision", INT, SEQUENCE),
            column("previous_id", DIGEST, CONTENT_HASH),
            column("payload_id", DIGEST, CONTENT_HASH),
            column("kind", INT, ENUM_CODE),
        ],
        provenance: Provenance::Records(&[RecordKind::Dependency]),
        purpose: "Immutable dependency envelopes; native validation remains required.",
    },
];

/// The table with this name, if the schema declares one.
#[must_use]
pub fn table(name: &str) -> Option<&'static Table> {
    TABLES.iter().find(|candidate| candidate.name == name)
}

/// The table names a block of SQL creates, in the order it creates them.
///
/// A deliberately small reader rather than a SQL parser: it strips `--` line comments, splits on
/// `;`, and recognises `CREATE TABLE <name>` at the start of a statement. That is enough to hold
/// the migration SQL and [`TABLES`] against each other, and it is stated as the lint it is — a
/// `CREATE TABLE` written in a shape this does not recognise would be missed, and a `--` inside a
/// string literal would be stripped as a comment. The migrations keep to one shape and carry no
/// string literal, and `tests/reconstruction.rs` checks the correspondence in both directions, so
/// a reader that stopped recognising a statement turns a test red rather than passing quietly.
#[must_use]
pub fn table_names_in_ddl(sql: &str) -> Vec<String> {
    let uncommented: String = sql
        .lines()
        .map(|line| match line.find("--") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut names = Vec::new();
    for statement in uncommented.split(';') {
        let mut words = statement.split_whitespace();
        if words.next().map(str::to_ascii_uppercase).as_deref() != Some("CREATE") {
            continue;
        }
        if words.next().map(str::to_ascii_uppercase).as_deref() != Some("TABLE") {
            continue;
        }
        if let Some(name) = words.next() {
            let cleaned = name.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            if !cleaned.is_empty() {
                names.push(cleaned.to_owned());
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_name_is_unique() {
        let mut seen: Vec<&str> = TABLES.iter().map(|table| table.name).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "two tables share a name");
    }

    #[test]
    fn every_column_name_is_unique_within_its_table() {
        for entry in TABLES {
            let mut names = entry.column_names();
            let before = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), before, "{} repeats a column name", entry.name);
        }
    }

    /// The rule, stated as a test rather than as a paragraph: no table is declared without a
    /// source, and only the migration ledger may be sourced from something other than records.
    #[test]
    fn only_the_migration_ledger_is_not_a_fold_over_records() {
        let ledgers: Vec<&str> = TABLES
            .iter()
            .filter(|table| table.provenance == Provenance::MigrationLedger)
            .map(|table| table.name)
            .collect();
        assert_eq!(ledgers, vec!["schema_version"]);

        for entry in TABLES {
            if entry.provenance == Provenance::MigrationLedger {
                continue;
            }
            assert!(
                !entry.provenance.record_kinds().is_empty(),
                "{} folds no record kind, so nothing can rebuild it",
                entry.name
            );
        }
    }

    #[test]
    fn the_ddl_reader_finds_a_create_table() {
        let sql = "CREATE TABLE first (a INTEGER) STRICT;\n\
                   CREATE UNIQUE INDEX idx ON first (a);\n\
                   CREATE TABLE second (b BLOB) STRICT;";
        assert_eq!(table_names_in_ddl(sql), vec!["first", "second"]);
    }

    #[test]
    fn the_ddl_reader_ignores_indexes_views_and_other_statements() {
        let sql = "CREATE INDEX one ON t (a);\nCREATE VIEW two AS SELECT 1;\nDROP TABLE three;";
        assert!(table_names_in_ddl(sql).is_empty());
    }

    /// The migrations open with a comment block explaining themselves, and a reader that did not
    /// strip comments would see `--` where it expected `CREATE` and miss the first table in every
    /// file. That is not hypothetical: it is what this reader did until `tests/reconstruction.rs`
    /// checked the correspondence in the second direction.
    #[test]
    fn the_ddl_reader_strips_line_comments() {
        let sql =
            "-- a heading\n-- CREATE TABLE decoy (x);\nCREATE TABLE real (x INTEGER) STRICT;\n\
                   CREATE TABLE second (y BLOB) STRICT; -- trailing\n";
        assert_eq!(table_names_in_ddl(sql), vec!["real", "second"]);
    }

    /// The real migrations, held against the real declarations. The same check lives in
    /// `tests/reconstruction.rs` in both directions; this one is here so a broken reader fails at
    /// `cargo test -p mesh-store --lib` too.
    #[test]
    fn the_reader_finds_every_declared_table_in_the_real_migrations() {
        // An upgrade can recreate an existing table to change a constraint. Compare the
        // declared identities, not the number of CREATE statements across all versions.
        let found = table_names_in_ddl(&crate::migration::full_schema_sql())
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        for table in TABLES {
            assert!(
                found.contains(&table.name.to_owned()),
                "the reader missed `{}` in the real migrations: {found:?}",
                table.name
            );
        }
        assert_eq!(found.len(), TABLES.len());
    }

    #[test]
    fn a_domain_that_mirrors_nothing_says_so() {
        assert_eq!(SEQUENCE.mesh_types_item, None);
        assert_eq!(OPERATION_ID.mesh_types_item, Some("ChangeSetId"));
    }
}
