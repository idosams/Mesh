-- 0002_review_and_context — the two remaining entries in plan §6.1's storage list.
--
-- Separate from 0001 because 0001 is the set the commit sequence needs to report PRIVATE_SAVED,
-- and this is the set review and the context ledger need. A database created before this migration
-- is a working local store; it just cannot answer a review question yet. That ordering is also
-- what gives the migration harness a real thing to test: 0002 runs against a database 0001's
-- commit path has already populated.
--
-- Additive only. Forward-only migrations mean this file may never be edited once it has run
-- anywhere — `schema_version.sql_fingerprint` is what catches an edit, rather than trusting that
-- nobody will.

CREATE TABLE review_bundle (
  bundle_id            BLOB NOT NULL PRIMARY KEY CHECK (length(bundle_id) = 32),
  subject_operation_id BLOB NOT NULL CHECK (length(subject_operation_id) = 32),
  opened_by_actor_id   BLOB NOT NULL CHECK (length(opened_by_actor_id) = 32),
  FOREIGN KEY (subject_operation_id) REFERENCES operation (operation_id)
) STRICT;

CREATE INDEX review_bundle_by_subject
  ON review_bundle (subject_operation_id);

-- A withdrawal is a later envelope with verdict 2, never an UPDATE of the envelope it withdraws.
-- The CHECK enumerates the codes `ReviewVerdict::code` produces, so a fourth verdict added in Rust
-- without a migration is rejected by SQLite instead of stored as an integer nothing reads.
CREATE TABLE review_approval (
  approval_id       BLOB    NOT NULL PRIMARY KEY CHECK (length(approval_id) = 32),
  bundle_id         BLOB    NOT NULL CHECK (length(bundle_id) = 32),
  approver_actor_id BLOB    NOT NULL CHECK (length(approver_actor_id) = 32),
  verdict           INTEGER NOT NULL CHECK (verdict IN (0, 1, 2)),
  FOREIGN KEY (bundle_id) REFERENCES review_bundle (bundle_id)
) STRICT;

CREATE INDEX review_approval_by_bundle
  ON review_approval (bundle_id);

CREATE TABLE context_ledger (
  entry_id     BLOB    NOT NULL PRIMARY KEY CHECK (length(entry_id) = 32),
  session_id   BLOB    NOT NULL CHECK (length(session_id) = 16),
  operation_id BLOB    NOT NULL CHECK (length(operation_id) = 32),
  access       INTEGER NOT NULL CHECK (access IN (0, 1, 2)),
  byte_length  INTEGER NOT NULL CHECK (byte_length >= 0),
  FOREIGN KEY (operation_id) REFERENCES operation (operation_id)
) STRICT;

CREATE INDEX context_ledger_by_session
  ON context_ledger (session_id);
