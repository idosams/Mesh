-- Immutable dependency envelopes. Native payload/authority verification is a separate gate.
-- The SQLite index is disposable; all rows reconstruct from required journal kind 8.
CREATE TABLE dependency_record (
  authority_id BLOB NOT NULL CHECK (length(authority_id) = 32),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  previous_id BLOB NOT NULL CHECK (length(previous_id) = 32),
  payload_id BLOB NOT NULL UNIQUE CHECK (length(payload_id) = 32),
  kind INTEGER NOT NULL CHECK (kind IN (0, 1, 2, 3, 4)),
  PRIMARY KEY (authority_id, revision)
) STRICT;
