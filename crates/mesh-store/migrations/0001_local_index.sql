-- 0001_local_index — the tables plan §6.3's commit sequence needs to report PRIVATE_SAVED.
--
-- Every table here is a fold over immutable records; `crates/mesh-store/src/schema.rs` declares
-- which records, and `tests/reconstruction.rs` refuses a table that declares none.
--
-- Every table is STRICT. That is not decoration: without it SQLite would accept the string
-- "hello" in a BLOB column and the length CHECK would pass on its five characters. STRICT is what
-- makes "this column holds thirty-two bytes of digest" a thing the database enforces rather than a
-- thing the writer remembers. It requires SQLite 3.37 or newer, which is the floor this schema
-- sets.
--
-- Every identifier column carries its own length CHECK. A digest that arrives truncated is a
-- corrupt index, and the whole recovery story is "drop it and rebuild", so the cheapest place to
-- notice is at the INSERT.

CREATE TABLE schema_version (
  version         INTEGER NOT NULL PRIMARY KEY CHECK (version > 0),
  sql_fingerprint BLOB    NOT NULL CHECK (length(sql_fingerprint) = 16)
) STRICT;

-- Plan §6.1: "canonical immutable operations should be stored as immutable rows with stable
-- hashes". Immutable means no column here is ever UPDATEd; a correction is a later operation.
CREATE TABLE operation (
  operation_id   BLOB    NOT NULL PRIMARY KEY CHECK (length(operation_id) = 32),
  actor_id       BLOB    NOT NULL CHECK (length(actor_id) = 32),
  actor_sequence INTEGER NOT NULL CHECK (actor_sequence >= 0),
  hlc_millis     INTEGER NOT NULL CHECK (hlc_millis >= 0),
  hlc_counter    INTEGER NOT NULL CHECK (hlc_counter >= 0),
  policy_epoch   INTEGER NOT NULL CHECK (policy_epoch >= 0),
  session_id     BLOB    NOT NULL CHECK (length(session_id) = 16),
  payload_digest BLOB    NOT NULL CHECK (length(payload_digest) = 32)
) STRICT;

-- One actor never issues two operations at the same sequence number. A UNIQUE index rather than a
-- convention, because the gap this detects — a fork in one actor's own chain — is the thing the
-- sync engine cannot repair silently.
CREATE UNIQUE INDEX operation_by_actor_sequence
  ON operation (actor_id, actor_sequence);

CREATE TABLE operation_parent (
  operation_id BLOB    NOT NULL CHECK (length(operation_id) = 32),
  ordinal      INTEGER NOT NULL CHECK (ordinal >= 0),
  parent_id    BLOB    NOT NULL CHECK (length(parent_id) = 32),
  PRIMARY KEY (operation_id, ordinal),
  FOREIGN KEY (operation_id) REFERENCES operation (operation_id)
) STRICT;

CREATE INDEX operation_parent_by_parent
  ON operation_parent (parent_id);

CREATE TABLE manifest (
  manifest_id    BLOB    NOT NULL PRIMARY KEY CHECK (length(manifest_id) = 32),
  byte_length    INTEGER NOT NULL CHECK (byte_length >= 0),
  content_digest BLOB    NOT NULL CHECK (length(content_digest) = 32)
) STRICT;

CREATE TABLE manifest_chunk (
  manifest_id  BLOB    NOT NULL CHECK (length(manifest_id) = 32),
  ordinal      INTEGER NOT NULL CHECK (ordinal >= 0),
  chunk_digest BLOB    NOT NULL CHECK (length(chunk_digest) = 32),
  byte_offset  INTEGER NOT NULL CHECK (byte_offset >= 0),
  byte_length  INTEGER NOT NULL CHECK (byte_length >= 0),
  PRIMARY KEY (manifest_id, ordinal),
  FOREIGN KEY (manifest_id) REFERENCES manifest (manifest_id)
) STRICT;

-- Plan §6.3 step 7. One row per actor, replaced as that actor's chain advances. Replaceable state
-- with an immutable source: the head is max(actor_sequence) over `operation`.
CREATE TABLE actor_head (
  actor_id       BLOB    NOT NULL PRIMARY KEY CHECK (length(actor_id) = 32),
  operation_id   BLOB    NOT NULL CHECK (length(operation_id) = 32),
  actor_sequence INTEGER NOT NULL CHECK (actor_sequence >= 0),
  FOREIGN KEY (operation_id) REFERENCES operation (operation_id)
) STRICT;

CREATE TABLE peer (
  peer_id             BLOB NOT NULL PRIMARY KEY CHECK (length(peer_id) = 32),
  joined_at_operation BLOB NOT NULL CHECK (length(joined_at_operation) = 32)
) STRICT;

CREATE TABLE peer_watermark (
  peer_id        BLOB    NOT NULL CHECK (length(peer_id) = 32),
  actor_id       BLOB    NOT NULL CHECK (length(actor_id) = 32),
  actor_sequence INTEGER NOT NULL CHECK (actor_sequence >= 0),
  PRIMARY KEY (peer_id, actor_id),
  FOREIGN KEY (peer_id) REFERENCES peer (peer_id)
) STRICT;

-- Plan §6.1's durable outbox. Held as a difference, not as a queue: a row is present exactly while
-- an operation's actor_sequence is above the acknowledging peer's watermark. Nothing deletes from
-- here on send — an acknowledgement record arrives, the watermark moves, and the row stops being
-- derivable. That is what makes the outbox survive losing this file.
CREATE TABLE outbox (
  peer_id        BLOB    NOT NULL CHECK (length(peer_id) = 32),
  actor_id       BLOB    NOT NULL CHECK (length(actor_id) = 32),
  actor_sequence INTEGER NOT NULL CHECK (actor_sequence >= 0),
  operation_id   BLOB    NOT NULL CHECK (length(operation_id) = 32),
  PRIMARY KEY (peer_id, operation_id),
  FOREIGN KEY (peer_id) REFERENCES peer (peer_id),
  FOREIGN KEY (operation_id) REFERENCES operation (operation_id)
) STRICT;

CREATE INDEX outbox_by_peer_sequence
  ON outbox (peer_id, actor_id, actor_sequence);
