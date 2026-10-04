//! Forward-only migrations, and the ledger that makes "forward-only" checkable.
//!
//! # Forward-only is a property of the type, not a promise in a comment
//!
//! [`Migration`] has one SQL field. There is no `down`, no `revert`, no `rollback_sql`, so a
//! reverse migration cannot be written, let alone run. That is the whole enforcement, and it is
//! stronger than a runtime refusal because a runtime refusal can be bypassed by calling something
//! else.
//!
//! # An applied migration is immutable, and that is checked
//!
//! Forward-only fails quietly in one way: somebody edits `0001_local_index.sql` after it has run
//! on a real database. Nothing about "we only go forwards" catches that — version 1 is still
//! version 1, and every future database gets a schema the old ones never received.
//!
//! So [`MigrationPlan`] writes a fingerprint of the exact SQL it ran into `schema_version`, and
//! [`verify_applied`] holds the recorded fingerprints against the current [`MIGRATIONS`]. A
//! rewritten migration is then a loud [`MigrationError::Rewritten`] on the next open, naming the
//! version. `tests/migrations.rs` proves it fires by editing a migration's text and reopening.

use crate::digest::{Digest16, DigestWriter, Fnv1a128};
use crate::row::{Row, Value};

/// One forward step of the schema.
///
/// Deliberately has no reverse. Plan §6.3's recovery for a broken index is "rebuild from immutable
/// operations and manifests", which is a better answer than a down-migration and does not need
/// one: there is nothing in this database whose loss is not recoverable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Migration {
    /// The version this migration produces. Contiguous from 1.
    pub version: u32,
    /// A short `[a-z0-9_]` name, used in errors and in the file name.
    pub name: &'static str,
    /// The SQL, exactly as it runs.
    pub sql: &'static str,
}

impl Migration {
    /// The fingerprint of this migration's SQL, as `schema_version` records it.
    #[must_use]
    pub fn fingerprint(&self) -> Digest16 {
        let mut writer = DigestWriter::<Fnv1a128>::new();
        writer.text(self.name);
        writer.text(self.sql);
        writer.finish()
    }
}

/// Every migration, in version order. Append only.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "local_index",
        sql: include_str!("../migrations/0001_local_index.sql"),
    },
    Migration {
        version: 2,
        name: "review_and_context",
        sql: include_str!("../migrations/0002_review_and_context.sql"),
    },
    Migration {
        version: 3,
        name: "dependency_records",
        sql: include_str!("../migrations/0003_dependency_records.sql"),
    },
];

/// The version a freshly opened database ends up at.
pub const CURRENT_VERSION: u32 = MIGRATIONS.len() as u32;

/// What went wrong when planning or verifying migrations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrationError {
    /// The database is at a version this build has never heard of — it was written by a newer
    /// build. Forward-only means this build must not touch it.
    FromTheFuture {
        /// The version the database reports.
        found: u32,
        /// The newest version this build knows.
        latest: u32,
    },
    /// A migration that has already run does not match the SQL this build carries for that
    /// version.
    Rewritten {
        /// The version whose SQL changed.
        version: u32,
        /// What the database recorded when it ran.
        recorded: Digest16,
        /// What this build would run now.
        expected: Digest16,
    },
    /// The ledger skips a version, so some migration never ran.
    Gap {
        /// The version that should have been recorded.
        expected: u32,
        /// What was recorded instead.
        found: u32,
    },
    /// [`MIGRATIONS`] itself is malformed. A build error in the shape of a value, caught by this
    /// crate's own tests before any database sees it.
    Malformed {
        /// What is wrong.
        reason: &'static str,
    },
}

impl core::fmt::Display for MigrationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FromTheFuture { found, latest } => write!(
                formatter,
                "the database is at schema version {found} and this build knows up to {latest}; \
                 forward-only migrations cannot open it"
            ),
            Self::Rewritten {
                version,
                recorded,
                expected,
            } => write!(
                formatter,
                "migration {version} has already run with fingerprint {recorded}, but this build \
                 carries {expected}; an applied migration is immutable"
            ),
            Self::Gap { expected, found } => write!(
                formatter,
                "the migration ledger jumps from {} to {found}, so migration {expected} never ran",
                expected.saturating_sub(1)
            ),
            Self::Malformed { reason } => {
                write!(formatter, "the migration set is malformed: {reason}")
            }
        }
    }
}

impl std::error::Error for MigrationError {}

/// One applied migration, as `schema_version` holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppliedMigration {
    /// The version.
    pub version: u32,
    /// The fingerprint recorded when it ran.
    pub fingerprint: Digest16,
}

/// The migrations still to run, and the rows recording them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationPlan {
    from_version: u32,
    steps: Vec<&'static Migration>,
}

impl MigrationPlan {
    /// The migrations this plan runs, in order.
    #[must_use]
    pub fn steps(&self) -> &[&'static Migration] {
        &self.steps
    }

    /// Whether there is nothing to do.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The version the database was at before this plan.
    #[must_use]
    pub const fn from_version(&self) -> u32 {
        self.from_version
    }

    /// The version the database is at once this plan has run.
    ///
    /// An empty plan leaves the database where it already was, which is why this is not the
    /// version of the last step alone.
    #[must_use]
    pub fn target_version(&self) -> u32 {
        self.steps
            .last()
            .map_or(self.from_version, |migration| migration.version)
    }

    /// The SQL to execute, in order: each migration's own text, then the ledger row recording it.
    ///
    /// The caller wraps the whole sequence in one transaction. A migration that half-ran and half
    /// recorded itself is the one state the ledger cannot describe, and SQLite's transaction is
    /// what stops it existing.
    #[must_use]
    pub fn statements(&self) -> Vec<String> {
        let mut statements = Vec::with_capacity(self.steps.len() * 2);
        for migration in &self.steps {
            statements.push(migration.sql.to_owned());
            statements.push(format!(
                "INSERT INTO schema_version (version, sql_fingerprint) VALUES ({}, {});",
                migration.version,
                Value::blob(migration.fingerprint().as_bytes()).to_sql_literal(),
            ));
        }
        statements
    }

    /// The ledger rows this plan writes, in [`crate::TABLES`] row order, for the reconstruction
    /// digest.
    #[must_use]
    pub fn ledger_rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = self
            .steps
            .iter()
            .map(|migration| {
                Row::new(vec![
                    Value::Integer(i64::from(migration.version)),
                    Value::blob(migration.fingerprint().as_bytes()),
                ])
            })
            .collect();
        rows.sort();
        rows
    }
}

/// The migrations to run against a database currently at `current_version`.
///
/// `current_version` is 0 for a database that has never been migrated.
///
/// # Errors
///
/// [`MigrationError::FromTheFuture`] when the database is newer than this build.
pub fn plan_migrations(current_version: u32) -> Result<MigrationPlan, MigrationError> {
    if current_version > CURRENT_VERSION {
        return Err(MigrationError::FromTheFuture {
            found: current_version,
            latest: CURRENT_VERSION,
        });
    }
    Ok(MigrationPlan {
        from_version: current_version,
        steps: MIGRATIONS
            .iter()
            .filter(|migration| migration.version > current_version)
            .collect(),
    })
}

/// Hold the ledger a database reports against the migrations this build carries.
///
/// # Errors
///
/// [`MigrationError::Gap`] when a version is missing from the ledger, [`MigrationError::Rewritten`]
/// when a migration's SQL has changed since it ran, and [`MigrationError::FromTheFuture`] when the
/// ledger holds a version this build does not know.
pub fn verify_applied(applied: &[AppliedMigration]) -> Result<u32, MigrationError> {
    let mut sorted: Vec<AppliedMigration> = applied.to_vec();
    sorted.sort_by_key(|entry| entry.version);

    for (index, entry) in sorted.iter().enumerate() {
        let expected = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if entry.version != expected {
            return Err(MigrationError::Gap {
                expected,
                found: entry.version,
            });
        }
        let Some(migration) = MIGRATIONS.iter().find(|m| m.version == entry.version) else {
            return Err(MigrationError::FromTheFuture {
                found: entry.version,
                latest: CURRENT_VERSION,
            });
        };
        let fingerprint = migration.fingerprint();
        if fingerprint != entry.fingerprint {
            return Err(MigrationError::Rewritten {
                version: entry.version,
                recorded: entry.fingerprint,
                expected: fingerprint,
            });
        }
    }

    Ok(u32::try_from(sorted.len()).unwrap_or(u32::MAX))
}

/// Every `CREATE TABLE` in every migration, concatenated in version order.
///
/// The schema a fresh database ends up with, as text — used by `tests/reconstruction.rs` to hold
/// [`crate::TABLES`] against the SQL from both sides.
#[must_use]
pub fn full_schema_sql() -> String {
    MIGRATIONS
        .iter()
        .map(|migration| migration.sql)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_contiguous_from_one() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(migration.version, u32::try_from(index + 1).unwrap());
        }
        assert_eq!(CURRENT_VERSION, MIGRATIONS.len() as u32);
    }

    /// Names are inlined into no SQL today, but they are inlined into error text and file names,
    /// and a name with a quote in it is the start of the class of bug this crate removed by having
    /// no text columns. Keeping the alphabet narrow keeps it removed.
    #[test]
    fn names_use_a_narrow_alphabet() {
        for migration in MIGRATIONS {
            assert!(
                !migration.name.is_empty()
                    && migration.name.bytes().all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'),
                "migration {} has an unusable name {:?}",
                migration.version,
                migration.name
            );
        }
    }

    #[test]
    fn a_fresh_database_runs_every_migration() {
        let plan = plan_migrations(0).expect("a fresh database plans");
        assert_eq!(plan.steps().len(), MIGRATIONS.len());
        assert_eq!(plan.target_version(), CURRENT_VERSION);
    }

    #[test]
    fn an_up_to_date_database_runs_nothing_and_stays_where_it_is() {
        let plan = plan_migrations(CURRENT_VERSION).expect("an up-to-date database plans");
        assert!(plan.is_empty());
        assert!(plan.statements().is_empty());
        assert_eq!(plan.target_version(), CURRENT_VERSION);
        assert_eq!(plan.from_version(), CURRENT_VERSION);
    }

    #[test]
    fn a_partly_migrated_database_runs_only_the_rest() {
        let plan = plan_migrations(1).expect("a v1 database plans");
        assert_eq!(plan.steps().len(), MIGRATIONS.len() - 1);
        assert_eq!(plan.steps()[0].version, 2);
        assert_eq!(plan.from_version(), 1);
        assert_eq!(plan.target_version(), CURRENT_VERSION);
    }

    /// Forward-only, at the one place a runtime check is still needed: this build must not write
    /// to a database a newer build created.
    #[test]
    fn a_database_from_the_future_is_refused() {
        assert_eq!(
            plan_migrations(CURRENT_VERSION + 1),
            Err(MigrationError::FromTheFuture {
                found: CURRENT_VERSION + 1,
                latest: CURRENT_VERSION,
            })
        );
    }

    #[test]
    fn every_step_writes_its_own_ledger_row() {
        let plan = plan_migrations(0).expect("plans");
        let statements = plan.statements();
        assert_eq!(statements.len(), MIGRATIONS.len() * 2);
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(statements[index * 2], migration.sql);
            assert!(statements[index * 2 + 1].contains("INSERT INTO schema_version"));
            assert!(statements[index * 2 + 1].contains(&format!("({},", migration.version)));
        }
    }

    #[test]
    fn a_complete_ledger_verifies() {
        let applied: Vec<AppliedMigration> = MIGRATIONS
            .iter()
            .map(|migration| AppliedMigration {
                version: migration.version,
                fingerprint: migration.fingerprint(),
            })
            .collect();
        assert_eq!(verify_applied(&applied), Ok(CURRENT_VERSION));
    }

    #[test]
    fn an_empty_ledger_is_version_zero() {
        assert_eq!(verify_applied(&[]), Ok(0));
    }

    #[test]
    fn a_gap_in_the_ledger_is_refused() {
        let applied = [AppliedMigration {
            version: 2,
            fingerprint: MIGRATIONS[1].fingerprint(),
        }];
        assert_eq!(
            verify_applied(&applied),
            Err(MigrationError::Gap {
                expected: 1,
                found: 2
            })
        );
    }

    /// The failure "forward-only" alone does not catch: an already-applied migration whose text
    /// changed. The fingerprint is what turns it into an error instead of a silent divergence.
    #[test]
    fn a_rewritten_migration_is_refused() {
        let applied = [AppliedMigration {
            version: 1,
            fingerprint: Digest16::from_bytes([0; 16]),
        }];
        let error = verify_applied(&applied).expect_err("a rewritten migration is refused");
        assert!(matches!(
            error,
            MigrationError::Rewritten { version: 1, .. }
        ));
        assert!(error.to_string().contains("immutable"));
    }

    #[test]
    fn a_ledger_version_this_build_does_not_know_is_refused() {
        let mut applied: Vec<AppliedMigration> = MIGRATIONS
            .iter()
            .map(|migration| AppliedMigration {
                version: migration.version,
                fingerprint: migration.fingerprint(),
            })
            .collect();
        applied.push(AppliedMigration {
            version: CURRENT_VERSION + 1,
            fingerprint: Digest16::from_bytes([9; 16]),
        });
        assert!(matches!(
            verify_applied(&applied),
            Err(MigrationError::FromTheFuture { .. })
        ));
    }

    /// Two migrations with the same text would share a fingerprint, and the rewrite check would
    /// then pass a swap of one for the other.
    #[test]
    fn fingerprints_are_distinct_across_migrations() {
        let mut seen: Vec<Digest16> = MIGRATIONS.iter().map(Migration::fingerprint).collect();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), before);
    }

    /// The name is absorbed as well as the SQL, so two migrations that happened to carry identical
    /// SQL would still be distinguishable.
    #[test]
    fn the_name_is_part_of_the_fingerprint() {
        let first = Migration {
            version: 1,
            name: "one",
            sql: "SELECT 1;",
        };
        let second = Migration {
            version: 1,
            name: "two",
            sql: "SELECT 1;",
        };
        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn the_full_schema_holds_every_migration() {
        let sql = full_schema_sql();
        for migration in MIGRATIONS {
            assert!(sql.contains(migration.sql));
        }
    }

    #[test]
    fn ledger_rows_are_sorted_and_two_wide() {
        let rows = plan_migrations(0).expect("plans").ledger_rows();
        assert_eq!(rows.len(), MIGRATIONS.len());
        assert!(rows.iter().all(|row| row.len() == 2));
        let mut sorted = rows.clone();
        sorted.sort();
        assert_eq!(rows, sorted);
    }
}
