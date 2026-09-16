//! The actor-key isolation attack harness: every Ed25519 software-custody path that could claim to
//! advance canonical state, walked from the software fallback and shown blocked. The separate
//! P-256 Secure Enclave approval path is covered by `secure_enclave_ffi.rs` and daemon receipt
//! tests; it is intentionally not described as absent here.
//!
//! The contract names this suite `--test key-isolation`; cargo derives a test target's name from
//! the file name, and a file called `key-isolation.rs` is not a legal Rust module path, so the
//! target is `key_isolation` and the command is
//! `cargo nextest run -p mesh-keychain --test key_isolation`.
//!
//! Each test is an *attack*, not a property. The distinction matters: a property test says the
//! system is well formed, and an attack says a specific thing an adversary would try does not
//! work. `docs/threat-model.md` A0 is the adversary — an agent with full user privileges, able to
//! read the workspace, run code the user allowed, drive the UI and observe memory it is given.

use mesh_crypto::{
    Capability, CustodyBackend, CustodyError, CustodyRequirement, DelegatedAction, Delegation,
    DelegationBudget, DomainSeparator, Ed25519, Expiry, ForActor, HumanAction, IsolationClass,
    KeyCustody, PolicyEpoch, SignatureScheme, SigningPayload, WorkspaceScope,
};
use mesh_keychain::{
    reachable_if_every_backend_were_built, reachable_today, KeyStoreSupport, Platform,
    SoftwareActorCustody, KEY_STORE_SUPPORT,
};

/// **A1.** The agent holds the software custody and asks it for the key.
///
/// There is no method to call. The assertion has to be about the source text, because the thing
/// being asserted is an absence: a test cannot call a function that does not exist, so a test that
/// *compiles* proves nothing about what is missing. This reads the crate's public surface for
/// every spelling of the accessor a lane would add.
///
/// **This is the test-time control; `src/no_export.rs` is the build-time one.** That module's
/// `const _: () = assert!(…)` is evaluated while the crate compiles, so it cannot be deselected;
/// this test can be, and in exchange it names the offending line and is exercised against
/// synthetic sources by [`the_test_time_scan_rejects_what_it_claims_to`], so a scan that quietly
/// stopped matching is caught. **Neither is a proof.** Both read text: an alias
/// (`use ed25519_dalek::SigningKey as Inner;`), a macro that assembles the signature, or a
/// re-export through another crate walks past both, and neither says anything about a debugger.
#[test]
fn no_public_method_in_this_crate_hands_a_caller_the_private_half() {
    for (name, source) in CRATE_SOURCES {
        let offences = export_shaped_lines(source);
        assert!(
            offences.is_empty(),
            "{name} exposes the private half: {offences:?}"
        );
    }
}

/// The scan the attack above runs, exercised against sources written for it.
///
/// A scan asserted only against the real tree passes for two different reasons — the tree is clean,
/// or the scan stopped matching — and cannot tell them apart.
#[test]
fn the_test_time_scan_rejects_what_it_claims_to() {
    for source in [
        // The spellings the banned-word list already caught.
        "pub fn secret_bytes(&self) -> [u8; 32] { self.0 }",
        "pub fn raw(&self) -> [u8; 32] { self.signing.to_bytes() }",
        // The four measured as missed by both scanners before this change.
        "pub fn signer(&self) -> &SigningKey {",
        "pub fn signer(&self) -> ed25519_dalek::SigningKey {",
        "impl AsRef<SigningKey> for SoftwareCustody<P> {",
        "impl Deref<Target = SigningKey> for SoftwareCustody<P> {",
        // …and the same four written the way Rust actually spells them. `Deref` in particular has
        // no `Target` in its `impl` header, so only the `type` rule reaches it.
        "impl Borrow<SigningKey> for SoftwareCustody<P> {",
        "impl Into<SigningKey> for SoftwareCustody<P> {",
        "    type Target = SigningKey;",
        "pub signing: SigningKey,",
        "pub use ed25519_dalek::SigningKey;",
        "pub(crate) fn signing_key(&self) -> &SigningKey {",
    ] {
        assert_eq!(
            export_shaped_lines(source).len(),
            1,
            "the test-time scan missed: {source}"
        );
    }
}

/// **The bypass that survived the first widening**, and the reason the unit is a header.
///
/// Measured on the line-at-a-time rule: every case below returned no offence from either scanner,
/// and `cargo fmt --check` accepted all of them — this is `rustfmt`'s own output for a signature
/// past the width limit, not a shape a lane has to contrive.
#[test]
fn a_header_split_across_lines_is_not_a_way_out() {
    for source in [
        "pub fn signer_for_the_currently_enrolled_actor(\n    &self,\n) -> &ed25519_dalek::SigningKey {",
        "impl<P: KeyPurpose>\n    AsRef<SigningKey> for SoftwareCustody<P>\n{",
        "pub use ed25519_dalek::{\n    Signature, SigningKey, VerifyingKey,\n};",
        "pub fn signer<P>(&self) -> Result<\n    &SigningKey,\n    CustodyError,\n> {",
        // A banned identifier on a continuation line of a wrapped `pub fn` header. Under the
        // line-at-a-time rule the offending line did not start with `pub fn`, so nothing read it.
        "pub fn material(\n    &self,\n    private_half: bool,\n) -> [u8; 32] {",
    ] {
        assert_eq!(
            export_shaped_lines(source).len(),
            1,
            "the header scan missed: {source}"
        );
    }
}

/// A body is outside every window, which is the whole of why rule 2 is usable on this crate.
#[test]
fn a_window_closes_at_the_end_of_its_header_and_does_not_read_the_body() {
    for source in [
        "pub struct SoftwareCustody<P: KeyPurpose> {\n    signing: SigningKey,\n}",
        "impl<P: KeyPurpose> SoftwareCustody<P> {\n    fn generate() {\n        let signing = SigningKey::from_bytes(&seed);\n    }\n}",
        "pub use crate::support::{\n    reachable_today, KeyStoreSupport, Platform,\n};",
    ] {
        assert!(
            export_shaped_lines(source).is_empty(),
            "the header scan fired on correct code: {source}"
        );
    }
}

/// The exclusion that makes the type-name rule usable: holding a `SigningKey` is this crate's job.
///
/// Banning the type outright would be a lint that fires on correct code, which is why the rule is
/// over the three line shapes where the name can only mean export.
#[test]
fn the_test_time_scan_does_not_fire_on_holding_the_key() {
    for source in [
        "use ed25519_dalek::{Signer as _, SigningKey};",
        "    signing: SigningKey,",
        "        let signing = SigningKey::from_bytes(&seed);",
        "impl<P: KeyPurpose> KeyCustody<P> for SoftwareCustody<P> {",
        "    implemented: bool,",
        "pub fn reachable_today(platform: Platform) -> IsolationClass {",
    ] {
        assert!(
            export_shaped_lines(source).is_empty(),
            "the test-time scan fired on correct code: {source}"
        );
    }
}

/// Every source of the crate under test, so the two scans below cannot drift apart from it.
const CRATE_SOURCES: [(&str, &str); 5] = [
    ("lib.rs", include_str!("../src/lib.rs")),
    (
        "secure_enclave.rs",
        include_str!("../src/secure_enclave.rs"),
    ),
    ("software.rs", include_str!("../src/software.rs")),
    ("entropy.rs", include_str!("../src/entropy.rs")),
    ("support.rs", include_str!("../src/support.rs")),
];

/// The identifiers that may not appear in a `pub fn` signature, whatever the types involved.
///
/// **Matched case-sensitively, and that is a stated limit rather than an oversight of this pass.**
/// A return type spelled `SecretBytes` is not `secret`, and rule 2 does not fire on it either
/// because it is not `SigningKey`. Folding the case here would fire on `CustodyError::Private`-style
/// names that are not exports, so the answer is not a casing change to this list — it is that the
/// guarantee rests on `KeyCustody` having no export method, and this scan is defence in depth over
/// that. `no_export.rs` carries the full bypass list.
const BANNED_IN_A_PUBLIC_SIGNATURE: [&str; 8] = [
    "secret",
    "private",
    "seed",
    "scalar",
    "export",
    "signing_key",
    "to_bytes",
    "into_bytes",
];

/// `SigningKey`, with casing and every non-alphanumeric character removed.
const SIGNING_KEY: &str = "signingkey";

/// Every line of `source` that hands a caller the private half.
///
/// Two rules, and **the unit of both is the item header rather than the line**:
///
/// 1. A `pub fn` signature naming any of [`BANNED_IN_A_PUBLIC_SIGNATURE`].
/// 2. A `pub …`, `impl …` or `type …` header naming `SigningKey` in any casing, with any path
///    prefix, by value or by reference. `ed25519_dalek::SigningKey::to_bytes` returns the 32-byte
///    seed, so `&SigningKey` is a full export. The `impl` class covers `AsRef`, `Borrow` and
///    `Into`, which are export paths not spelled `pub fn` at all; the `type` class covers `Deref`,
///    whose `impl` header never names the target — `type Target = SigningKey;` does, on its own
///    line.
///
/// The header window is what closed the bypass measured after the first widening:
/// `pub fn f(\n    &self,\n) -> &SigningKey {` names the type on a line that opens with `)`, which
/// no rule over single lines can attribute to a `pub fn`, and it is what `rustfmt` emits for any
/// signature past the width limit rather than something a lane has to contrive. A header opens at
/// `pub`, `impl` or `type` and closes at the `{` or `;` that ends it — except for a fully public
/// `use`, whose braces are a group, so only `;` closes it — with [`HEADER_WINDOW_LINES`] as the
/// bound. Bodies are outside every window, which is what keeps `signing: SigningKey` from firing.
fn export_shaped_lines(source: &str) -> Vec<String> {
    let mut offences = Vec::new();
    let mut inside_header = false;
    let mut public_use = false;
    let mut signature = false;
    let mut spanned = 0usize;

    for line in source.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if !inside_header && opens_an_item_header(line) {
            inside_header = true;
            public_use = opens_a_public_use(line);
            signature = line.starts_with("pub fn");
            spanned = 0;
        }
        if !inside_header {
            continue;
        }
        let names_the_type = normalised(line).contains(SIGNING_KEY);
        let names_a_banned_identifier = signature
            && BANNED_IN_A_PUBLIC_SIGNATURE
                .iter()
                .any(|banned| line.contains(banned));
        if names_the_type || names_a_banned_identifier {
            offences.push(line.to_owned());
        }
        spanned += 1;
        if closes_an_item_header(line, public_use) || spanned >= HEADER_WINDOW_LINES {
            inside_header = false;
        }
    }
    offences
}

/// The most lines an item header may span before the window closes regardless.
///
/// A bound rather than an open window: a header that never reaches its terminator is not legal
/// Rust, and letting one run on would turn rule 2 into the outright ban on `SigningKey` that this
/// crate cannot have, because holding one is its job.
const HEADER_WINDOW_LINES: usize = 16;

/// Whether a line opens one of the three item headers in which the type name can only mean export.
///
/// The whole-word test is what keeps `implemented: bool` in `support.rs` from reading as an `impl`.
fn opens_an_item_header(line: &str) -> bool {
    ["pub", "impl", "type"]
        .into_iter()
        .any(|word| opens_with_word(line, word))
}

/// Whether the header is a fully public `use`, whose braces are a group and not a body.
///
/// `pub(crate) use` is deliberately not matched: a crate-private re-export is not reachable from
/// outside the crate, so it is not an export path.
fn opens_a_public_use(line: &str) -> bool {
    opens_with_word(line, "pub") && opens_with_word(line["pub".len()..].trim_start(), "use")
}

/// Whether `line` begins with `word` as a whole word.
fn opens_with_word(line: &str, word: &str) -> bool {
    line.strip_prefix(word)
        .is_some_and(|rest| !rest.starts_with(|next: char| next.is_alphanumeric() || next == '_'))
}

/// Whether this line ends the item header opened earlier.
fn closes_an_item_header(line: &str, public_use: bool) -> bool {
    if public_use {
        return line.contains(';');
    }
    line.contains('{') || line.contains(';') || line.contains('}')
}

/// `line` with its casing folded and every non-alphanumeric character removed.
fn normalised(line: &str) -> String {
    line.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// **A2.** The agent implements `HumanKeyCustody` inside this crate to mint itself an attestation.
///
/// The empty `impl HumanKeyCustody for X {}` used to be enough — the trait's `attest_human` had a
/// default body that asked nothing. It no longer is, and no source here implements the trait at
/// all. Both halves are asserted: the second because a future one would inherit the first.
#[test]
fn no_source_in_this_crate_implements_human_key_custody() {
    for (name, source) in [
        ("lib.rs", include_str!("../src/lib.rs")),
        (
            "secure_enclave.rs",
            include_str!("../src/secure_enclave.rs"),
        ),
        ("software.rs", include_str!("../src/software.rs")),
        ("entropy.rs", include_str!("../src/entropy.rs")),
        ("support.rs", include_str!("../src/support.rs")),
    ] {
        let flattened: String = source
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("//"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !flattened.contains("HumanKeyCustody for"),
            "{name} implements HumanKeyCustody"
        );
    }
}

/// **A3.** The agent asks the software backend for the isolation proof directly.
///
/// The proof is the argument `HumanKeyAttestation` needs, and there is no value of it to be had
/// from a backend below the floor. This is the link that makes the rest unreachable.
#[test]
fn a_software_backend_cannot_obtain_the_proof_a_human_attestation_requires() {
    let custody = SoftwareActorCustody::generate().expect("os entropy");
    assert_eq!(custody.backend(), CustodyBackend::SoftwareInProcess);
    assert_eq!(custody.backend().isolation(), IsolationClass::InProcess);
    assert_eq!(custody.backend().isolation_proof(), None);
    assert_eq!(
        CustodyRequirement::HUMAN_APPROVAL.check(custody.backend()),
        Err(CustodyError::InsufficientIsolation {
            required: IsolationClass::OsMediated,
            offered: IsolationClass::InProcess,
        })
    );
}

/// **A4.** The agent takes the actor key it *can* have and delegates itself publication authority.
///
/// The delegation vocabulary has no variant for it. This is the `mesh-crypto` property restated at
/// the point where a real key finally exists, because "an agent cannot name canonical advancement"
/// was previously only ever asserted against a key nobody held.
#[test]
fn a_real_software_key_still_cannot_name_canonical_head_advancement() {
    let custody = SoftwareActorCustody::generate().expect("os entropy");
    let agent = SoftwareActorCustody::generate().expect("os entropy");

    let delegation = Delegation::new(
        agent.public_key(),
        [
            DelegatedAction::AuthorChangeSet,
            DelegatedAction::ReadWorkspace,
        ],
        Expiry::at_unix_millis(2_000),
    );
    // The vocabulary the delegation draws from is `DelegatedAction`, and every action a
    // `Delegation` can carry is one of its variants. There is no `AdvanceCanonicalHead` among
    // them, so the hostile argument cannot be written rather than being rejected when passed.
    for action in delegation.actions() {
        assert!(
            !action.as_str().contains("canonical"),
            "a delegable action names canonical state: {}",
            action.as_str()
        );
    }
    // And the human vocabulary, which does contain it, is not what a `Delegation` carries: the
    // two are different types with no conversion, so the widening cannot be written either.
    assert_eq!(
        HumanAction::AdvanceCanonicalHead.as_str(),
        "advance-canonical-head"
    );
    assert_eq!(DelegatedAction::parse("advance-canonical-head"), None);
    let _ = custody;
}

/// **A5.** The agent uses the software key to sign an approval payload anyway.
///
/// It can. The signature is valid Ed25519 over those bytes, and it is **not an approval**: it can
/// satisfy neither the legacy `HumanHeld` capability nor the current ES256 credential and typed
/// receipt. The control is not that the agent cannot sign; it is that an actor signature has the
/// wrong authority and algorithm for shared publication.
#[test]
fn an_agent_can_sign_bytes_and_still_holds_nothing_that_admits_a_transition() {
    let agent = SoftwareActorCustody::generate().expect("os entropy");
    let payload = SigningPayload::new(
        DomainSeparator::new("mesh.v0.approval-envelope"),
        b"expected-head=00",
    );
    let signature = agent.sign(&payload).expect("a software key signs");
    Ed25519::verify(
        &agent.public_key().public_key(),
        payload.as_bytes(),
        &signature,
    )
    .expect("it is a real signature");

    // …and the capability that would let it mean anything is unreachable.
    assert_eq!(agent.backend().isolation_proof(), None);
}

/// **A6.** The agent asks for a hardware-isolated custody and takes whatever it is given.
///
/// A resolver that downgrades is the ordinary way this fails in the field: the enclave is absent,
/// the caller gets a software store, and everything keeps working while protecting nothing. The
/// requirement refuses.
#[test]
fn requiring_isolation_refuses_rather_than_downgrading() {
    let strict = CustodyRequirement::at_least(IsolationClass::HardwareNonExportable);
    for backend in [
        CustodyBackend::SoftwareInProcess,
        CustodyBackend::AppleKeychain,
        CustodyBackend::LinuxKernelKeyring,
        CustodyBackend::WindowsCng,
    ] {
        assert!(strict.check(backend).is_err(), "{backend} was admitted");
    }
    assert!(strict.check(CustodyBackend::AppleSecureEnclave).is_ok());
    assert!(strict.check(CustodyBackend::HardwareToken).is_ok());
}

/// **A7.** The agent reads a log, a `Debug` render or a support bundle for key material.
///
/// Nothing this crate can be asked to render carries the private half. Asserted against the actual
/// bytes: the test generates a key, renders every diagnostic surface, and searches for the public
/// half's hex (which is allowed) and for any 64-character hex run that is not it (which would be a
/// scalar).
#[test]
fn no_diagnostic_surface_renders_anything_that_is_not_the_public_half() {
    let custody = SoftwareActorCustody::generate().expect("os entropy");
    let public = custody.public_key().to_hex();

    let payload = SigningPayload::new(DomainSeparator::CAPABILITY_TOKEN, b"body");
    let rendered = [
        format!("{custody:?}"),
        format!("{payload:?}"),
        format!("{:?}", custody.backend()),
        format!("{}", custody.backend()),
        format!("{:?}", CustodyError::BackendUnavailable),
        CustodyError::InsufficientIsolation {
            required: IsolationClass::OsMediated,
            offered: IsolationClass::InProcess,
        }
        .to_string(),
    ];

    for text in rendered {
        for run in hex_runs(&text) {
            assert_eq!(
                run, public,
                "a 64-hex run that is not the public key: {text}"
            );
        }
        assert!(!text.contains('/'), "a diagnostic quoted a path: {text}");
    }
}

/// The signature of `Capability::<HumanHeld>::root`, named so the reference below stays readable.
///
/// Naming it is the point of the binding: if the attestation argument ever stopped being required,
/// this alias would stop matching and the test would fail to compile.
type RootMint = fn(
    &mesh_crypto::HumanKeyAttestation,
    WorkspaceScope,
    [HumanAction; 1],
    PolicyEpoch,
    Expiry,
    DelegationBudget,
) -> Capability<mesh_crypto::HumanHeld>;

/// Every maximal run of 64 or more hex characters in `text`, truncated to 64.
fn hex_runs(text: &str) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = String::new();
    for character in text.chars().chain(core::iter::once(' ')) {
        if character.is_ascii_hexdigit() {
            current.push(character);
        } else {
            if current.len() >= 64 {
                runs.push(current.chars().take(64).collect());
            }
            current.clear();
        }
    }
    runs
}

/// **A8.** A reader takes `GUARANTEES.md` at its word.
///
/// The document and `support.rs` are two statements of the same survey, and a document that drifts
/// from the code is worse than no document: it is a claim with the appearance of evidence. Every
/// row's mechanism name and reachable class must appear in the file.
#[test]
fn the_guarantee_document_agrees_with_the_table_it_documents() {
    let document = include_str!("../GUARANTEES.md");
    let crate_docs = include_str!("../src/lib.rs");
    for row in KEY_STORE_SUPPORT {
        let line = format!(
            "| {} | `{}` |",
            row.platform().as_str(),
            row.backend().as_str()
        );
        assert!(
            document.contains(&line),
            "GUARANTEES.md has no row for {} / {}",
            row.platform(),
            row.backend()
        );
    }
    for class in [
        IsolationClass::InProcess,
        IsolationClass::OsGated,
        IsolationClass::OsMediated,
        IsolationClass::HardwareNonExportable,
    ] {
        assert!(
            document.contains(&format!("`{}`", class.as_str())),
            "GUARANTEES.md never names the {class} class"
        );
    }
    assert!(
        document.contains("cannot hold an Ed25519 key"),
        "GUARANTEES.md no longer states the Secure Enclave constraint"
    );
    assert!(
        document.contains("can extract this key"),
        "GUARANTEES.md no longer states the software fallback's limit"
    );
    for text in [document, crate_docs] {
        assert!(
            text.contains("separate P-256") || text.contains("deliberately separate"),
            "the actor-key guarantee no longer names the separate human-approval path"
        );
        assert!(
            !text.contains("nothing built on it can"),
            "an obsolete actor-key absolute denies the implemented human-approval path"
        );
    }
}

/// **A9.** A reader assumes hardware backing because `CustodyBackend::AppleSecureEnclave` exists.
///
/// The mechanism is hardware-isolated and cannot hold the key, and only the second fact decides
/// what a Mesh user gets. Both are asserted here so that flipping either one fails.
#[test]
fn no_platform_reaches_hardware_isolation_for_an_actor_key_without_an_external_token() {
    for platform in Platform::ALL {
        assert_eq!(
            reachable_today(platform),
            IsolationClass::InProcess,
            "{platform} claims more than the software fallback"
        );
        assert_eq!(
            reachable_if_every_backend_were_built(platform),
            IsolationClass::HardwareNonExportable
        );
    }

    let enclave = KEY_STORE_SUPPORT
        .iter()
        .find(|row| row.backend() == CustodyBackend::AppleSecureEnclave)
        .expect("a Secure Enclave row");
    assert!(enclave.backend().is_hardware_isolated());
    assert!(!enclave.holds_ed25519());
    assert_eq!(enclave.reachable_isolation(), IsolationClass::InProcess);

    for row in KEY_STORE_SUPPORT
        .iter()
        .filter(|row| row.reachable_isolation() == IsolationClass::HardwareNonExportable)
    {
        assert_eq!(row.backend(), CustodyBackend::HardwareToken);
    }
    let _: fn(&KeyStoreSupport) -> IsolationClass = KeyStoreSupport::reachable_isolation;
}

/// **A10.** The human tier, checked from this side of the seam.
///
/// `HumanAction` is the only legacy capability vocabulary with `AdvanceCanonicalHead` in it, and a
/// capability at that tier needs an attestation the software actor-key backend cannot produce. The
/// current ES256 receipt is a separate authority path and cannot be substituted with this key.
#[test]
fn the_only_tier_that_may_publish_needs_a_value_this_crate_cannot_build() {
    let custody = SoftwareActorCustody::generate().expect("os entropy");

    // Everything below is constructible from a software key.
    let _scope = WorkspaceScope::from_bytes([7; 16]);
    let _epoch = PolicyEpoch::new(1);
    let _budget = DelegationBudget::new(1);
    let _expiry = Expiry::at_unix_millis(1_000);
    let _key: mesh_crypto::KeyPair<ForActor> = custody.public_key();

    // `Capability::<HumanHeld>::root` needs a `HumanKeyAttestation`. There is no expression of
    // type `HumanKeyAttestation` reachable from here: its only constructor is `pub(crate)` in
    // `mesh-crypto` and takes an `IsolationProof`, whose only constructor answers this backend
    // with `None`. The line below is what stands in for the call that cannot be written.
    assert_eq!(custody.backend().isolation_proof(), None);
    assert_eq!(
        HumanAction::AdvanceCanonicalHead.as_str(),
        "advance-canonical-head"
    );
    let _: RootMint = Capability::root;
}
