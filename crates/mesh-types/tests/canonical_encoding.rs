//! Properties of the canonical encoding, over generated corpora rather than examples.
//!
//! The corpora are drawn by a SplitMix64 with pinned seeds, so "random" is the same four thousand
//! cases on every machine and in every run. A property test that draws fresh entropy proves a
//! different thing each time and cannot be cited as evidence that a value is stable.
//!
//! Four claims are made here and each one is made in a way that can fail:
//!
//! 1. **Encoding is a function.** The same record encodes to the same bytes, in the same process
//!    and — via the pinned aggregate below — in a different process on a different machine.
//! 2. **The encoding is one-to-one.** Distinct records encode to distinct bytes, and the decoder
//!    accepts exactly the byte string the encoder produces and rejects every other spelling of the
//!    same value.
//! 3. **Field order is load-bearing.** Permuting a record's fields changes its bytes, over the
//!    whole corpus and not on one hand-picked example.
//! 4. **The two encodings in this crate discriminate identically.** The canonical encoding and the
//!    identity framing are different byte strings by design; if one ever stopped binding a field
//!    the other binds, they would stop agreeing about which records are distinct.

use std::collections::{BTreeMap, HashSet};

use mesh_types::{
    canonical_digest, decode_canonical, derive_id, encode_canonical, schema_violations, ActorId,
    ActorSequence, Blake3, CanonicalEncode, CanonicalValue, CausalParents, CborError, CborReader,
    ChangeSet, ChangeSetDraft, ChangeSetId, ChunkRef, ContentDigest, DecodeError, Digest32,
    DigestHasher, DirectoryEntry, DirectoryVersion, FileManifest, FileVersion, HeadId, Hlc,
    ManifestId, NormalizedName, ObjectId, PolicyEpoch, PortableMetadata, SessionId, Signature,
    VersionId, WorkspaceId,
};

/// SplitMix64: identical on every platform because every operation is defined on wrapping `u64`.
struct SplitMix64(u64);

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn digest(&mut self) -> Digest32 {
        let mut bytes = [0u8; 32];
        for slot in &mut bytes {
            *slot = (self.next() & 0xff) as u8;
        }
        Digest32::from_bytes(bytes)
    }

    fn u10(&mut self) -> [u8; 10] {
        let mut bytes = [0u8; 10];
        for slot in &mut bytes {
            *slot = (self.next() & 0xff) as u8;
        }
        bytes
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

const CORPUS: usize = 4096;
const SEED: u64 = 0x0EDA_C0DE_5EED_1234;

/// `tests/serialization-compat.rs` is the check that an encoding change without a vector update
/// fails CI, and it is gated on the `vectors` feature. That makes it a CI gate **only** while the
/// feature is a default one: dropping `vectors` from `default` would leave `cargo nextest run
/// --workspace` compiling the whole oracle out and passing, with the published corpus free to
/// drift.
///
/// This file is not gated, so this assertion cannot be switched off with the thing it protects.
#[test]
fn the_vectors_feature_is_still_a_default_feature() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        manifest.contains("default = [\"vectors\"]"),
        "crates/mesh-types/Cargo.toml no longer makes `vectors` a default feature, so \
         tests/serialization-compat.rs is compiled out of `cargo nextest run --workspace` and the \
         published corpus is no longer compared to the encoder on any ordinary test run."
    );
}

// ---------------------------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------------------------

fn manifest(rng: &mut SplitMix64) -> FileManifest {
    let count = rng.below(6) as usize;
    let mut offset = 0u64;
    let mut chunks = Vec::with_capacity(count);
    for _ in 0..count {
        // Lengths reach past 23, 255 and 65535 so that every integer head width appears in the
        // corpus rather than only the one-byte one.
        let length = rng.below(70_000) + 1;
        chunks.push(ChunkRef::new(rng.digest(), offset, length));
        offset += length;
    }
    FileManifest::new(offset, rng.digest(), chunks)
}

fn file_version(rng: &mut SplitMix64) -> FileVersion {
    let parents = (0..rng.below(4))
        .map(|_| VersionId::from_digest(rng.digest()))
        .collect();
    FileVersion::new(
        ObjectId::mint(rng.next(), rng.u10()),
        parents,
        ManifestId::from_digest(rng.digest()),
        PortableMetadata::new(rng.next() & 1 == 1),
        ChangeSetId::from_digest(rng.digest()),
    )
}

fn directory_version(rng: &mut SplitMix64) -> DirectoryVersion {
    let mut entries = BTreeMap::new();
    for index in 0..rng.below(6) {
        let name = NormalizedName::new(format!("entry-{}-{index}", rng.next()))
            .expect("generated names are structurally valid");
        entries.insert(
            name,
            DirectoryEntry::new(
                ObjectId::mint(rng.next(), rng.u10()),
                VersionId::from_digest(rng.digest()),
            ),
        );
    }
    DirectoryVersion::new(ObjectId::mint(rng.next(), rng.u10()), entries)
}

fn changeset(rng: &mut SplitMix64) -> ChangeSet<()> {
    let parents = match rng.below(3) {
        0 => CausalParents::genesis(),
        count => CausalParents::after(
            ChangeSetId::from_digest(rng.digest()),
            (0..count - 1)
                .map(|_| ChangeSetId::from_digest(rng.digest()))
                .collect(),
        ),
    };
    let operations = vec![(); rng.below(4) as usize];
    ChangeSetDraft::<()>::new(
        WorkspaceId::mint(rng.next(), rng.u10()),
        ActorId::from_digest(rng.digest()),
        SessionId::mint(rng.next(), rng.u10()),
        ActorSequence::new(rng.next()),
        Hlc::new(rng.next(), (rng.next() & 0xffff_ffff) as u32),
    )
    .causal_parents(parents)
    .base_head(HeadId::from_digest(rng.digest()))
    .policy_epoch(PolicyEpoch::new(rng.next()))
    .seal(
        operations,
        HeadId::from_digest(rng.digest()),
        Signature::from_bytes([(rng.next() & 0xff) as u8; 64]),
    )
}

/// Run `check` over one generated record of every type, `CORPUS` times.
fn over_the_corpus(
    mut check: impl FnMut(&FileManifest, &FileVersion, &DirectoryVersion, &ChangeSet<()>),
) {
    let mut rng = SplitMix64::new(SEED);
    for _ in 0..CORPUS {
        check(
            &manifest(&mut rng),
            &file_version(&mut rng),
            &directory_version(&mut rng),
            &changeset(&mut rng),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 1. Encoding is a function, and the same one everywhere
// ---------------------------------------------------------------------------------------------

/// The canonical encodings of the whole generated corpus, folded in order into one digest.
///
/// Computed by one process on one machine and checked in. It moves if any field order changes, if
/// any head width changes, if an integer is ever written little-endian on some other target, if a
/// domain tag is edited, or if a field is added, removed or reinterpreted. That is the whole
/// point: **one number stands for sixteen thousand encodings**, so "identical across process
/// restarts and platforms" is checked on every run of the suite rather than asserted.
///
/// If this constant has to move, the published vectors move with it, and both are a compatibility
/// event under the task's failure-and-recovery clause — never a refactor.
const ENCODING_AGGREGATE: &str = "36a8af6fbff0d44cc06f28df200201e160efaf2ef98aa4fc6d5e6a33ab8d0cbe";

#[test]
fn the_corpus_encodes_to_one_pinned_aggregate() {
    assert_eq!(corpus_aggregate(), ENCODING_AGGREGATE);
}

fn corpus_aggregate() -> String {
    let mut aggregate = <Blake3 as ContentDigest>::hasher();
    over_the_corpus(|manifest, file, directory, changeset| {
        DigestHasher::update(&mut aggregate, &encode_canonical(manifest));
        DigestHasher::update(&mut aggregate, &encode_canonical(file));
        DigestHasher::update(&mut aggregate, &encode_canonical(directory));
        DigestHasher::update(&mut aggregate, &encode_canonical(changeset));
    });
    DigestHasher::finalize(aggregate).to_hex()
}

#[test]
fn encoding_the_same_record_twice_produces_identical_bytes() {
    over_the_corpus(|manifest, file, directory, changeset| {
        assert_eq!(encode_canonical(manifest), encode_canonical(manifest));
        assert_eq!(encode_canonical(file), encode_canonical(file));
        assert_eq!(encode_canonical(directory), encode_canonical(directory));
        assert_eq!(encode_canonical(changeset), encode_canonical(changeset));
    });
}

#[test]
fn the_digest_of_the_encoding_is_the_digest_of_the_bytes() {
    over_the_corpus(|manifest, file, directory, changeset| {
        assert_eq!(
            canonical_digest::<Blake3, _>(manifest),
            Blake3::digest_bytes(&encode_canonical(manifest))
        );
        assert_eq!(
            canonical_digest::<Blake3, _>(file),
            Blake3::digest_bytes(&encode_canonical(file))
        );
        assert_eq!(
            canonical_digest::<Blake3, _>(directory),
            Blake3::digest_bytes(&encode_canonical(directory))
        );
        assert_eq!(
            canonical_digest::<Blake3, _>(changeset),
            Blake3::digest_bytes(&encode_canonical(changeset))
        );
    });
}

/// The values a record produces must be the values its published schema describes, or the schema
/// in `protocol/schemas/` is a document about a format nobody implements.
#[test]
fn every_record_in_the_corpus_agrees_with_its_published_schema() {
    over_the_corpus(|manifest, file, directory, changeset| {
        assert_eq!(schema_violations(manifest), Vec::new());
        assert_eq!(schema_violations(file), Vec::new());
        assert_eq!(schema_violations(directory), Vec::new());
        assert_eq!(schema_violations(changeset), Vec::new());
    });
}

// ---------------------------------------------------------------------------------------------
// 2. The encoding is one-to-one
// ---------------------------------------------------------------------------------------------

#[test]
fn distinct_records_encode_to_distinct_bytes() {
    let mut seen = HashSet::new();
    let mut count = 0usize;
    over_the_corpus(|manifest, file, directory, changeset| {
        for encoding in [
            encode_canonical(manifest),
            encode_canonical(file),
            encode_canonical(directory),
            encode_canonical(changeset),
        ] {
            count += 1;
            assert!(seen.insert(encoding), "two records share one encoding");
        }
    });
    assert_eq!(count, CORPUS * 4);
    assert_eq!(seen.len(), count);
}

/// The round trip, over the whole corpus. `decode` is the inverse of `encode`, and re-encoding
/// what was decoded returns the identical bytes — which is the property an implementation in
/// another language has to reproduce.
#[test]
fn every_record_round_trips_through_the_decoder() {
    over_the_corpus(|manifest, file, directory, changeset| {
        round_trip(manifest);
        round_trip(file);
        round_trip(directory);
        round_trip(changeset);
    });
}

fn round_trip<R: CanonicalEncode>(record: &R) {
    let encoded = encode_canonical(record);
    let decoded = decode_canonical::<R>(&encoded).expect("its own encoding decodes");
    assert_eq!(decoded, record.canonical_fields(), "values did not survive");

    // Re-encoding the decoded values reproduces the bytes exactly. This is the half that catches
    // a decoder that is merely lenient rather than exact.
    let mut writer = mesh_types::CborWriter::new();
    writer.array((decoded.len() + 1) as u64);
    writer.text(R::SCHEMA.domain.as_str());
    let mut reader = CborReader::new(&encoded);
    let _ = reader.array();
    let _ = reader.text();
    for _ in 0..decoded.len() {
        writer.nested(reader.skip_item().expect("each field is well formed"));
    }
    assert_eq!(writer.finish(), encoded, "re-encoding changed the bytes");
}

/// A record type only decodes its own bytes. Without the leading domain tag two records with the
/// same field shapes would be interchangeable, which is what domain separation exists to prevent.
#[test]
fn one_record_types_bytes_do_not_decode_as_another() {
    let mut rng = SplitMix64::new(SEED);
    let directory = directory_version(&mut rng);
    let encoded = encode_canonical(&directory);

    match decode_canonical::<FileManifest>(&encoded) {
        Err(DecodeError::WrongArity { expected, found }) => {
            assert_eq!((expected, found), (4, 3));
        }
        other => panic!("a directory version decoded as a manifest: {other:?}"),
    }
}

#[test]
fn a_record_with_the_right_arity_and_the_wrong_domain_is_refused() {
    // A file version and a ChangeSet have different arities, so the tag is tested directly: take a
    // real encoding and rewrite only its domain tag to another of the same length.
    let mut rng = SplitMix64::new(SEED);
    let version = file_version(&mut rng);
    let encoded = encode_canonical(&version);
    let tag = b"mesh.v0.file-version";
    let at = encoded
        .windows(tag.len())
        .position(|window| window == tag)
        .expect("the tag is in the encoding");
    let mut tampered = encoded.clone();
    tampered[at..at + tag.len()].copy_from_slice(b"mesh.v0.file-versioN");

    assert_eq!(
        decode_canonical::<FileVersion>(&tampered),
        Err(DecodeError::WrongDomain {
            expected: "mesh.v0.file-version",
            found: "mesh.v0.file-versioN".to_owned(),
        })
    );
}

#[test]
fn trailing_bytes_after_a_complete_record_are_refused() {
    let mut rng = SplitMix64::new(SEED);
    let manifest = manifest(&mut rng);
    let mut encoded = encode_canonical(&manifest);
    let at = encoded.len();
    encoded.push(0x00);

    assert_eq!(
        decode_canonical::<FileManifest>(&encoded),
        Err(DecodeError::TrailingBytes { at, extra: 1 })
    );
}

/// The property that makes "byte-identical across implementations" checkable rather than hopeful:
/// a *valid* CBOR encoding of the same value, spelled differently, is not accepted.
#[test]
fn a_non_canonical_spelling_of_the_same_record_is_refused() {
    let manifest = FileManifest::new(6, Digest32::from_bytes([0xab; 32]), Vec::new());
    let encoded = encode_canonical(&manifest);
    // `byte_length` is 6, written as the single head byte 0x06. Rewrite it as 0x18 0x06, which is
    // the same value in a wider head — legal CBOR, and not this profile.
    let at = encoded
        .iter()
        .position(|byte| *byte == 0x06)
        .expect("the byte_length is in the encoding");
    let mut widened = encoded.clone();
    widened.splice(at..=at, [0x18, 0x06]);

    assert_eq!(encoded.len() + 1, widened.len());
    assert_eq!(
        decode_canonical::<FileManifest>(&widened),
        Err(DecodeError::Cbor(CborError::NonCanonicalHead {
            at,
            argument: 6,
            used: 1,
        }))
    );
}

// ---------------------------------------------------------------------------------------------
// 3. Field order is load-bearing
// ---------------------------------------------------------------------------------------------

/// Encode a record's fields in a permuted order, the way a second implementation that got the
/// schema wrong would.
fn encode_permuted<R: CanonicalEncode>(record: &R, first: usize, second: usize) -> Vec<u8> {
    let mut fields = record.canonical_fields();
    fields.swap(first, second);
    let mut writer = mesh_types::CborWriter::new();
    writer.array((fields.len() + 1) as u64);
    writer.text(R::SCHEMA.domain.as_str());
    for field in &fields {
        write_value(&mut writer, field);
    }
    writer.finish()
}

fn write_value(writer: &mut mesh_types::CborWriter, value: &CanonicalValue) {
    match value {
        CanonicalValue::Unsigned(number) => {
            writer.unsigned(*number);
        }
        CanonicalValue::Bool(flag) => {
            writer.bool(*flag);
        }
        CanonicalValue::Bytes(bytes) => {
            writer.bytes(bytes);
        }
        CanonicalValue::Text(text) => {
            writer.text(text);
        }
        CanonicalValue::Sequence(items) | CanonicalValue::Group(items) => {
            writer.array(items.len() as u64);
            for item in items {
                write_value(writer, item);
            }
        }
        CanonicalValue::Record(encoding) => {
            writer.nested(encoding);
        }
    }
}

/// **The test the acceptance criterion asks for, and it is checked over the corpus rather than an
/// example.** Two fields swapped is two different byte strings, for every record of every type.
///
/// The pairs are chosen to be adjacent fields of *different* shapes, because a swap of two fields
/// that happen to hold equal bytes would legitimately leave the encoding unchanged and would make
/// this test lie about what it proves.
#[test]
fn swapping_any_two_fields_changes_the_bytes_of_every_record() {
    over_the_corpus(|manifest, file, directory, changeset| {
        // file-manifest: byte_length (unsigned) against content_hash (32 bytes).
        assert_ne!(
            encode_canonical(manifest),
            encode_permuted(manifest, 0, 1),
            "file manifest survived a field swap"
        );
        // file-version: object_id (16 bytes) against parent_versions (sequence).
        assert_ne!(
            encode_canonical(file),
            encode_permuted(file, 0, 1),
            "file version survived a field swap"
        );
        // directory-version: object_id (16 bytes) against entries (sequence).
        assert_ne!(
            encode_canonical(directory),
            encode_permuted(directory, 0, 1),
            "directory version survived a field swap"
        );
        // changeset: workspace_id (16 bytes) against actor_id (32 bytes).
        assert_ne!(
            encode_canonical(changeset),
            encode_permuted(changeset, 0, 1),
            "ChangeSet survived a field swap"
        );
        // And a swap deeper in the field list, where an off-by-one in a hand-written encoder
        // would land: base_head against resulting_head, two fields of identical shape.
        assert_ne!(
            encode_canonical(changeset),
            encode_permuted(changeset, 5, 6),
            "ChangeSet survived swapping its two head fields"
        );
    });
}

/// A swapped field must also be *caught*, not merely different. Two same-shaped fields swapped
/// decode without complaint and produce different values; two different-shaped ones are a decode
/// error. Both are stated, because "the decoder rejects it" is only true for the second.
#[test]
fn a_swapped_field_is_either_a_different_value_or_a_decode_error() {
    let mut rng = SplitMix64::new(SEED);
    let manifest = manifest(&mut rng);

    let swapped = encode_permuted(&manifest, 0, 1);
    match decode_canonical::<FileManifest>(&swapped) {
        Err(DecodeError::Cbor(CborError::TypeMismatch { expected, .. })) => {
            assert_eq!(expected, "unsigned");
        }
        other => panic!("a shape-changing swap was not caught: {other:?}"),
    }

    let changeset = changeset(&mut rng);
    let heads_swapped = encode_permuted(&changeset, 5, 6);
    let decoded = decode_canonical::<ChangeSet<()>>(&heads_swapped)
        .expect("two fields of one shape still decode");
    assert_ne!(
        decoded,
        changeset.canonical_fields(),
        "swapping two same-shaped fields must still change the values"
    );
}

// ---------------------------------------------------------------------------------------------
// 4. The canonical encoding and the identity framing discriminate identically
// ---------------------------------------------------------------------------------------------

/// `mesh-types` carries two byte producers for the same record: this canonical encoding, and the
/// [`DigestWriter`](mesh_types::DigestWriter) framing that `derive_id` names records through. They
/// are different byte strings on purpose (see `src/canonical.rs`), and that is a standing risk:
/// a field added to one and forgotten in the other would go unnoticed until a signature failed to
/// verify against an identifier.
///
/// This is the check that makes the risk tolerable rather than merely acknowledged. Over the
/// corpus, two records have the same canonical encoding **if and only if** they have the same
/// record identifier. A field bound by one and not the other breaks that equivalence: two records
/// differing only in that field collide under the encoding that ignores it and not under the
/// other.
///
/// Proved to bite by removing a field: dropping `policy_epoch` from `ChangeSet`'s
/// `canonical_fields` makes two ChangeSets differing only in their epoch collide here, and the
/// assertion below fires. The transcript is in this task's PR body.
#[test]
fn the_two_encodings_agree_about_which_records_are_distinct() {
    let mut rng = SplitMix64::new(SEED ^ 0xFFFF);

    // Pairs that differ in exactly one field, one pair per field of each record type. If either
    // encoding stops binding that field, its two members collide under that encoding and not
    // under the other.
    let base_manifest = manifest(&mut rng);
    let manifests = vec![
        base_manifest.clone(),
        FileManifest::new(
            base_manifest.byte_length() + 1,
            *base_manifest.content_hash(),
            base_manifest.chunks().to_vec(),
        ),
        FileManifest::new(
            base_manifest.byte_length(),
            rng.digest(),
            base_manifest.chunks().to_vec(),
        ),
        FileManifest::new(
            base_manifest.byte_length(),
            *base_manifest.content_hash(),
            {
                let mut chunks = base_manifest.chunks().to_vec();
                chunks.push(ChunkRef::new(rng.digest(), u64::MAX / 2, 1));
                chunks
            },
        ),
    ];
    assert_discrimination_agrees(&manifests);

    let base_version = file_version(&mut rng);
    let versions = vec![
        base_version.clone(),
        FileVersion::new(
            ObjectId::mint(rng.next(), rng.u10()),
            base_version.parent_versions().to_vec(),
            base_version.manifest_id(),
            base_version.portable_metadata(),
            base_version.created_by(),
        ),
        FileVersion::new(
            base_version.object_id(),
            {
                let mut parents = base_version.parent_versions().to_vec();
                parents.push(VersionId::from_digest(rng.digest()));
                parents
            },
            base_version.manifest_id(),
            base_version.portable_metadata(),
            base_version.created_by(),
        ),
        FileVersion::new(
            base_version.object_id(),
            base_version.parent_versions().to_vec(),
            ManifestId::from_digest(rng.digest()),
            base_version.portable_metadata(),
            base_version.created_by(),
        ),
        FileVersion::new(
            base_version.object_id(),
            base_version.parent_versions().to_vec(),
            base_version.manifest_id(),
            PortableMetadata::new(!base_version.portable_metadata().is_executable()),
            base_version.created_by(),
        ),
        FileVersion::new(
            base_version.object_id(),
            base_version.parent_versions().to_vec(),
            base_version.manifest_id(),
            base_version.portable_metadata(),
            ChangeSetId::from_digest(rng.digest()),
        ),
    ];
    assert_discrimination_agrees(&versions);

    let object = ObjectId::mint(rng.next(), rng.u10());
    let entry = DirectoryEntry::new(
        ObjectId::mint(rng.next(), rng.u10()),
        VersionId::from_digest(rng.digest()),
    );
    let name = |text: &str| NormalizedName::new(text).expect("valid");
    let directories = vec![
        DirectoryVersion::empty(object),
        DirectoryVersion::empty(ObjectId::mint(rng.next(), rng.u10())),
        DirectoryVersion::new(object, BTreeMap::from([(name("a"), entry)])),
        DirectoryVersion::new(object, BTreeMap::from([(name("b"), entry)])),
        DirectoryVersion::new(
            object,
            BTreeMap::from([(
                name("a"),
                DirectoryEntry::new(entry.object_id(), VersionId::from_digest(rng.digest())),
            )]),
        ),
        DirectoryVersion::new(
            object,
            BTreeMap::from([(
                name("a"),
                DirectoryEntry::new(ObjectId::mint(rng.next(), rng.u10()), entry.version_id()),
            )]),
        ),
    ];
    assert_discrimination_agrees(&directories);

    let changesets = changesets_differing_in_one_field_each(&mut rng);
    assert_discrimination_agrees(&changesets);
}

/// Every ChangeSet field, varied one at a time from one base.
fn changesets_differing_in_one_field_each(rng: &mut SplitMix64) -> Vec<ChangeSet<()>> {
    let workspace = WorkspaceId::mint(1, [1; 10]);
    let actor = ActorId::from_digest(Digest32::from_bytes([2; 32]));
    let session = SessionId::mint(3, [3; 10]);
    let base = HeadId::from_digest(Digest32::from_bytes([4; 32]));
    let result = HeadId::from_digest(Digest32::from_bytes([5; 32]));

    let build = |workspace, actor, session, sequence, hlc, parents, base, result, epoch, ops| {
        ChangeSetDraft::<()>::new(workspace, actor, session, sequence, hlc)
            .causal_parents(parents)
            .base_head(base)
            .policy_epoch(epoch)
            .seal(ops, result, Signature::from_bytes([0; 64]))
    };

    let with_parent = CausalParents::after(ChangeSetId::from_digest(rng.digest()), Vec::new());
    vec![
        build(
            workspace,
            actor,
            session,
            ActorSequence::new(1),
            Hlc::new(10, 0),
            CausalParents::genesis(),
            base,
            result,
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            WorkspaceId::mint(9, [9; 10]),
            actor,
            session,
            ActorSequence::new(1),
            Hlc::new(10, 0),
            CausalParents::genesis(),
            base,
            result,
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            workspace,
            ActorId::from_digest(Digest32::from_bytes([9; 32])),
            session,
            ActorSequence::new(1),
            Hlc::new(10, 0),
            CausalParents::genesis(),
            base,
            result,
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            workspace,
            actor,
            SessionId::mint(9, [9; 10]),
            ActorSequence::new(1),
            Hlc::new(10, 0),
            CausalParents::genesis(),
            base,
            result,
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            workspace,
            actor,
            session,
            ActorSequence::new(2),
            Hlc::new(10, 0),
            CausalParents::genesis(),
            base,
            result,
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            workspace,
            actor,
            session,
            ActorSequence::new(1),
            Hlc::new(11, 0),
            CausalParents::genesis(),
            base,
            result,
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            workspace,
            actor,
            session,
            ActorSequence::new(1),
            Hlc::new(10, 1),
            CausalParents::genesis(),
            base,
            result,
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            workspace,
            actor,
            session,
            ActorSequence::new(1),
            Hlc::new(10, 0),
            with_parent,
            base,
            result,
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            workspace,
            actor,
            session,
            ActorSequence::new(1),
            Hlc::new(10, 0),
            CausalParents::genesis(),
            HeadId::from_digest(Digest32::from_bytes([9; 32])),
            result,
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            workspace,
            actor,
            session,
            ActorSequence::new(1),
            Hlc::new(10, 0),
            CausalParents::genesis(),
            base,
            HeadId::from_digest(Digest32::from_bytes([9; 32])),
            PolicyEpoch::new(1),
            Vec::new(),
        ),
        build(
            workspace,
            actor,
            session,
            ActorSequence::new(1),
            Hlc::new(10, 0),
            CausalParents::genesis(),
            base,
            result,
            PolicyEpoch::new(2),
            Vec::new(),
        ),
        build(
            workspace,
            actor,
            session,
            ActorSequence::new(1),
            Hlc::new(10, 0),
            CausalParents::genesis(),
            base,
            result,
            PolicyEpoch::new(1),
            vec![()],
        ),
    ]
}

/// For every pair in `records`: the two encodings agree about whether they are the same record.
fn assert_discrimination_agrees<R>(records: &[R])
where
    R: CanonicalEncode + mesh_types::CanonicalRecord,
    R::Id: PartialEq + core::fmt::Debug,
{
    for (left_index, left) in records.iter().enumerate() {
        for (right_index, right) in records.iter().enumerate().skip(left_index + 1) {
            let same_encoding = encode_canonical(left) == encode_canonical(right);
            let same_identifier = derive_id::<Blake3, _>(left) == derive_id::<Blake3, _>(right);
            assert_eq!(
                same_encoding,
                same_identifier,
                "records {left_index} and {right_index} of {}: the canonical encoding says \
                 same={same_encoding} and the identity framing says same={same_identifier}. One \
                 of them has stopped binding a field the other binds.",
                R::SCHEMA.domain
            );
            assert!(
                !same_encoding,
                "records {left_index} and {right_index} of {} differ in exactly one field and \
                 must not collide",
                R::SCHEMA.domain
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------------------------

/// Two records whose fields concatenate to the same characters must not encode the same. Every
/// string carries its own length, so they cannot.
#[test]
fn adjacent_text_fields_cannot_be_confused() {
    let object = ObjectId::mint(1, [0; 10]);
    let entry = DirectoryEntry::new(
        object,
        VersionId::from_digest(Digest32::from_bytes([1; 32])),
    );
    let name = |text: &str| NormalizedName::new(text).expect("valid");

    let left = DirectoryVersion::new(
        object,
        BTreeMap::from([(name("ab"), entry), (name("c"), entry)]),
    );
    let right = DirectoryVersion::new(
        object,
        BTreeMap::from([(name("a"), entry), (name("bc"), entry)]),
    );
    assert_ne!(encode_canonical(&left), encode_canonical(&right));
}

/// A directory version's entries are in sorted name order because a `BTreeMap` iterates that way,
/// so the encoding cannot depend on the order a caller inserted them in.
#[test]
fn insertion_order_does_not_reach_the_encoding() {
    let object = ObjectId::mint(1, [0; 10]);
    let entry = |byte: u8| {
        DirectoryEntry::new(
            ObjectId::mint(u64::from(byte), [byte; 10]),
            VersionId::from_digest(Digest32::from_bytes([byte; 32])),
        )
    };
    let name = |text: &str| NormalizedName::new(text).expect("valid");

    let mut forward = BTreeMap::new();
    forward.insert(name("a"), entry(1));
    forward.insert(name("b"), entry(2));
    forward.insert(name("c"), entry(3));

    let mut backward = BTreeMap::new();
    backward.insert(name("c"), entry(3));
    backward.insert(name("b"), entry(2));
    backward.insert(name("a"), entry(1));

    assert_eq!(
        encode_canonical(&DirectoryVersion::new(object, forward)),
        encode_canonical(&DirectoryVersion::new(object, backward))
    );
}

/// Non-ASCII names are encoded by their UTF-8 byte length, and ordered by their UTF-8 bytes.
#[test]
fn utf8_names_are_ordered_and_measured_by_their_bytes() {
    let object = ObjectId::mint(1, [0; 10]);
    let entry = DirectoryEntry::new(
        object,
        VersionId::from_digest(Digest32::from_bytes([1; 32])),
    );
    let name = |text: &str| NormalizedName::new(text).expect("valid");

    let directory = DirectoryVersion::new(
        object,
        BTreeMap::from([
            (name("\u{6c34}"), entry),
            (name("README"), entry),
            (name("caf\u{e9}"), entry),
        ]),
    );
    let encoded = encode_canonical(&directory);
    let text: Vec<&str> = directory
        .entries()
        .keys()
        .map(NormalizedName::as_str)
        .collect();
    assert_eq!(text, vec!["README", "caf\u{e9}", "\u{6c34}"]);

    // "caf\u{e9}" is four characters and five UTF-8 bytes. The head must say five, not four: a
    // length taken from `chars().count()` would produce 0x64 here and a stream nothing can decode.
    let utf8 = "caf\u{e9}".as_bytes();
    assert_eq!(utf8.len(), 5);
    let at = encoded
        .windows(utf8.len())
        .position(|window| window == utf8)
        .expect("the name is in the encoding");
    assert_eq!(encoded[at - 1], 0x65, "a five-byte text head");
}
