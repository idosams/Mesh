-- Extend required dependency kinds without rewriting migration 3 or losing indexed history.
-- MigrationPlan runs this replacement and its ledger update in one transaction.
ALTER TABLE dependency_record RENAME TO dependency_record_before_consumption;
CREATE TABLE dependency_record (
  authority_id BLOB NOT NULL CHECK (length(authority_id) = 32),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  previous_id BLOB NOT NULL CHECK (length(previous_id) = 32),
  payload_id BLOB NOT NULL UNIQUE CHECK (length(payload_id) = 32),
  kind INTEGER NOT NULL CHECK (kind IN (0, 1, 2, 3, 4, 5, 6)),
  PRIMARY KEY (authority_id, revision)
) STRICT;
INSERT INTO dependency_record (authority_id, revision, previous_id, payload_id, kind)
SELECT authority_id, revision, previous_id, payload_id, kind
FROM dependency_record_before_consumption;
DROP TABLE dependency_record_before_consumption;
