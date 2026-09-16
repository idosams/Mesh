//! The property: **the only route from decoded field values to a `Capability` runs through a
//! signature that has already been checked.**
//!
//! `CapabilityParts` is deliberately harmless — anyone may build one, from any bytes, naming any
//! tier and any action. What separates those field values from *authority* is one thing:
//! `CapabilityToken::verify` checks the issuer's signature over exactly those bytes **first**, and
//! only then calls the `pub(crate)` `Capability::from_parts`. Turn that `pub(crate)` into `pub` and
//! the separation is gone: every crate in the workspace gains a way to mint a
//! `Capability<HumanHeld>` carrying `HumanAction::AdvanceCanonicalHead` out of bytes it chose.
//!
//! That fence was four characters of `pub(crate)` with nothing holding it up. This file holds it up
//! four ways, none of which trusts a route merely because its name looks safe:
//!
//! 1. [`no_crate_outside_this_one_can_turn_field_values_into_a_capability`] exercises the route from
//!    *outside* the library, where a widened `from_parts` changes which function the call resolves
//!    to and the file stops compiling.
//! 2. [`every_route_to_a_capability_is_one_of_the_five_this_crate_sanctions`] inventories every
//!    function in the crate that yields a `Capability` and pins the visibility each is declared at,
//!    so a *differently named* public mint fails too.
//! 3. [`hostile_field_values_become_a_capability_only_when_the_signature_checks`] shows the
//!    signature doing the separating: identical bytes, one signature that checks and one that does
//!    not, and only one of them is ever a capability.
//! 4. [`canonical_verification_is_only_a_checked_delegation`] proves the product wrapper reaches
//!    authority only through the generic signature-checking route, and demonstrates that an
//!    unchecked decode-and-construct mutation is rejected by the same assertion.

mod support;

use mesh_crypto::{
    ActorKey, AuthorityTier, Capability, CapabilityCodec, CapabilityParts, CapabilityToken,
    DelegationBudget, Expiry, HumanHeld, KeyCustody, PolicyEpoch, TokenError, VerifyError,
};

use support::{workspace, PlumbingCodec, PlumbingScheme, TestCustody};

const EPOCH: PolicyEpoch = PolicyEpoch::new(7);
const NOW: u64 = 1_000;
const HORIZON: Expiry = Expiry::at_unix_millis(10_000);

// ---------------------------------------------------------------------------------------------
// 1. The route, exercised from outside the library.
// ---------------------------------------------------------------------------------------------

/// What the call below evaluates to while the fence holds.
#[derive(Debug, PartialEq, Eq)]
struct BytesAreStillJustBytes;

/// A decoy `from_parts` for every `Capability<T>`, defined in *this* crate.
///
/// Rust prefers an **inherent** associated function over a trait one. While
/// `Capability::from_parts` is `pub(crate)` the inherent function is not nameable from here — an
/// integration test is a separate crate — so `Capability::<HumanHeld>::from_parts(&parts)` resolves
/// to this decoy and evaluates to [`BytesAreStillJustBytes`].
///
/// Make the inherent one `pub` and it wins the resolution instead. The call then evaluates to
/// `Result<Capability<HumanHeld>, PartsError>`, the binding below stops type-checking, and
/// `cargo test -p mesh-crypto` fails to build with `error[E0308]: mismatched types ... expected
/// BytesAreStillJustBytes, found Result<Capability<HumanHeld>, ...>`.
///
/// The property under the trick is not "the keyword `pub(crate)` is present". It is that **no crate
/// other than `mesh-crypto` can name a function that turns field values into a capability** —
/// which is what a foreign crate reaching this decoy, and only this decoy, demonstrates.
trait FieldValuesGrantNothing {
    fn from_parts(parts: &CapabilityParts) -> BytesAreStillJustBytes;
}

impl<T: AuthorityTier> FieldValuesGrantNothing for Capability<T> {
    fn from_parts(_parts: &CapabilityParts) -> BytesAreStillJustBytes {
        BytesAreStillJustBytes
    }
}

/// Field values a hostile caller would choose: the human tier, and publication authority.
fn hostile_parts(issuer: ActorKey, subject: ActorKey) -> CapabilityParts {
    CapabilityParts::new(
        issuer,
        subject,
        workspace(),
        "human-held".to_owned(),
        vec!["advance-canonical-head".to_owned()],
        EPOCH,
        HORIZON,
        DelegationBudget::new(1),
    )
}

#[test]
fn no_crate_outside_this_one_can_turn_field_values_into_a_capability() {
    let parts = hostile_parts(
        ActorKey::from_public_bytes([0x11; 32]),
        ActorKey::from_public_bytes([0x22; 32]),
    );

    // Resolves to the decoy above, because the inherent `Capability::from_parts` is `pub(crate)`
    // and this is a different crate. If it ever resolves to the inherent one, this does not compile.
    let outcome: BytesAreStillJustBytes = Capability::<HumanHeld>::from_parts(&parts);

    assert_eq!(
        outcome,
        BytesAreStillJustBytes,
        "a crate outside `mesh-crypto` reached a constructor that turns chosen field values into a \
         capability; the only route to one is `CapabilityToken::verify`, after the signature checks"
    );
}

// ---------------------------------------------------------------------------------------------
// 2. The inventory: every function in the crate that yields a `Capability`.
// ---------------------------------------------------------------------------------------------

/// Every source file of the crate. A route added in a new module is a route this scan must read,
/// and `every_module_of_the_crate_is_scanned` is what notices a module that is missing here.
const SOURCES: [(&str, &str); 12] = [
    ("lib.rs", include_str!("../src/lib.rs")),
    ("capability.rs", include_str!("../src/capability.rs")),
    (
        "conformance_impl.rs",
        include_str!("../src/conformance_impl.rs"),
    ),
    ("custody.rs", include_str!("../src/custody.rs")),
    ("domain.rs", include_str!("../src/domain.rs")),
    ("ed25519.rs", include_str!("../src/ed25519.rs")),
    ("keys.rs", include_str!("../src/keys.rs")),
    (
        "no_secret_material.rs",
        include_str!("../src/no_secret_material.rs"),
    ),
    ("parts.rs", include_str!("../src/parts.rs")),
    ("rotation.rs", include_str!("../src/rotation.rs")),
    ("scheme.rs", include_str!("../src/scheme.rs")),
    ("token.rs", include_str!("../src/token.rs")),
];

/// Every function in the crate that yields a `Capability`, with the visibility it is declared at
/// and the reason that visibility is the right one.
///
/// Five routes, and each one is safe for a stated reason:
///
/// | route | visibility | what guards it |
/// |---|---|---|
/// | `Capability::delegate` | `pub` | needs a parent capability, and every field is narrower |
/// | `Capability::from_parts` | `pub(crate)` | **unreachable outside the crate**; one caller |
/// | `Capability::<HumanHeld>::root` | `pub` | needs a `HumanKeyAttestation`, which needs custody |
/// | `CapabilityToken::verify` | `pub` | checks the issuer's signature over the bytes first |
/// | `CapabilityToken::verify_canonical` | `pub` | delegates to `verify` with the private canonical codec |
///
/// A public `from_parts` is a sixth route with **no** guard: bytes a caller chose, straight to a
/// `Capability<HumanHeld>` that names `AdvanceCanonicalHead`.
const SANCTIONED_ROUTES: [(&str, &str, &str); 5] = [
    ("capability.rs", "delegate", "pub"),
    ("capability.rs", "from_parts", "pub(crate)"),
    ("capability.rs", "root", "pub"),
    ("token.rs", "verify", "pub"),
    ("token.rs", "verify_canonical", "pub"),
];

/// `header` with a balanced leading `<…>` generic list removed.
fn skip_generics(header: &str) -> &str {
    let trimmed = header.trim_start();
    if !trimmed.starts_with('<') {
        return trimmed;
    }
    let mut depth = 0usize;
    for (at, character) in trimmed.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return &trimmed[at + 1..];
                }
            }
            _ => {}
        }
    }
    trimmed
}

/// The type an **inherent** `impl` header names, or `None` when it is a trait implementation.
fn inherent_impl_target(line: &str) -> Option<&str> {
    let rest = skip_generics(line.strip_prefix("impl")?);
    if rest.contains(" for ") {
        return None;
    }
    let head = rest.trim_start();
    let end = head
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(head.len());
    Some(&head[..end])
}

/// One `fn` declaration, flattened from however many lines it spans.
fn declaration_at(lines: &[&str], start: usize) -> String {
    let mut declaration = String::new();
    for line in &lines[start..] {
        let text = line.trim();
        declaration.push_str(text);
        declaration.push(' ');
        if text.contains('{') || text.ends_with(';') {
            break;
        }
    }
    declaration
}

/// The visibility a flattened declaration is written at.
fn visibility_of(declaration: &str) -> &'static str {
    let head = declaration.split("fn ").next().unwrap_or("");
    if head.contains("pub(crate)") {
        "pub(crate)"
    } else if head.contains("pub(super)") {
        "pub(super)"
    } else if head.trim_start().starts_with("pub") {
        "pub"
    } else {
        "private"
    }
}

/// The function's name.
fn name_of(declaration: &str) -> &str {
    let after = declaration
        .split_once("fn ")
        .map_or("", |(_, rest)| rest.trim_start());
    let end = after
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    &after[..end]
}

/// Whatever follows the first `-> ` in a flattened declaration.
fn return_type(declaration: &str) -> &str {
    declaration.split_once("-> ").map_or("", |(_, rest)| rest)
}

/// Every function in `source` that yields a `Capability`.
///
/// Two shapes count, because both are ways to hand one out: a return type that names
/// `Capability<…>` anywhere in it, and a `Self` return from inside an inherent `impl` of
/// `Capability`. `to_parts`, which returns `CapabilityParts`, is neither — it hands out field
/// values, and field values are the harmless direction.
fn routes_to_a_capability(file: &'static str, source: &str) -> Vec<(&'static str, String, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut routes = Vec::new();
    let mut inside_capability_impl = false;

    for (index, line) in lines.iter().enumerate() {
        if line.starts_with("impl") {
            inside_capability_impl = inherent_impl_target(line) == Some("Capability");
            continue;
        }
        if *line == "}" {
            inside_capability_impl = false;
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("//") || !trimmed.contains("fn ") {
            continue;
        }
        let declaration = declaration_at(&lines, index);
        let returns = return_type(&declaration);
        let yields_a_capability =
            returns.contains("Capability<") || (inside_capability_impl && returns.contains("Self"));
        if yields_a_capability {
            routes.push((
                file,
                name_of(&declaration).to_owned(),
                visibility_of(&declaration).to_owned(),
            ));
        }
    }
    routes
}

/// Every route in the crate, in source order.
fn every_route() -> Vec<(&'static str, String, String)> {
    SOURCES
        .iter()
        .flat_map(|(file, source)| routes_to_a_capability(file, source))
        .collect()
}

#[test]
fn every_route_to_a_capability_is_one_of_the_five_this_crate_sanctions() {
    let expected: Vec<(&str, String, String)> = SANCTIONED_ROUTES
        .iter()
        .map(|(file, name, visibility)| ((*file), (*name).to_owned(), (*visibility).to_owned()))
        .collect();

    assert_eq!(
        every_route(),
        expected,
        "the set of functions in `mesh-crypto` that yield a `Capability` changed. Each of the five \
         sanctioned routes is safe for its own stated reason (see SANCTIONED_ROUTES); a new one, \
         or an existing one widened, is a way to obtain authority that no reviewer has weighed. \
         `from_parts` in particular must stay `pub(crate)`: public, it mints a \
         `Capability<HumanHeld>` naming `AdvanceCanonicalHead` from bytes the caller chose"
    );
}

const CHECKED_CANONICAL_DELEGATION: &str =
    "self.verify::<S, CanonicalCapabilityCodec, T>(expected_issuer, presented_by, now, epoch)";

fn canonical_wrapper_is_checked(token_source: &str) -> bool {
    let Some(start) = token_source.find("pub fn verify_canonical<") else {
        return false;
    };
    token_source[start..].contains(CHECKED_CANONICAL_DELEGATION)
}

/// Both public token routes remain signature-before-construction: generic `verify` performs the
/// check before it calls `Capability::from_parts`, and the product wrapper does not decode or
/// construct anything independently — it delegates to that checked route with the private codec.
///
/// The in-memory mutation is the bypass this guard exists to kill. It replaces the exact checked
/// delegation with an unchecked decode-and-construct spelling; the predicate that accepts the real
/// source must reject the mutant in the same run.
#[test]
fn canonical_verification_is_only_a_checked_delegation() {
    let (_, token_source) = SOURCES
        .iter()
        .find(|(file, _)| *file == "token.rs")
        .expect("token.rs is scanned");

    let verify_start = token_source
        .find("pub fn verify<S:")
        .expect("the generic verified route remains public");
    let canonical_start = token_source
        .find("pub fn verify_canonical<")
        .expect("the canonical verified route remains public");
    let generic_verify = &token_source[verify_start..canonical_start];
    let signature_check = generic_verify
        .find("S::verify(")
        .expect("generic verification checks the signature");
    let capability_construction = generic_verify
        .find("Capability::<T>::from_parts")
        .expect("generic verification constructs authority only after its checks");
    assert!(
        signature_check < capability_construction,
        "the generic token route constructs a capability before checking the issuer's signature"
    );
    assert!(
        canonical_wrapper_is_checked(token_source),
        "`verify_canonical` no longer delegates through the signature-checking generic route"
    );

    let bypass = token_source.replace(
        CHECKED_CANONICAL_DELEGATION,
        "Capability::<T>::from_parts(&CanonicalCapabilityCodec::decode(&self.payload).expect(\"unchecked\"))",
    );
    assert_ne!(
        bypass.as_str(),
        *token_source,
        "the canonical delegation changed, so the planted bypass is a no-op"
    );
    assert!(
        !canonical_wrapper_is_checked(&bypass),
        "the authority-route guard accepted an unchecked canonical decode-and-construct bypass"
    );
}

/// The scan has teeth, demonstrated in the same run rather than remembered from a mutated tree.
///
/// This is the widening the task measured — `pub(crate) fn from_parts` to `pub fn from_parts` —
/// applied to a copy of the source in memory. It must show up as a public route, and the inventory
/// above must disagree with it.
#[test]
fn the_inventory_notices_the_widening_it_exists_to_notice() {
    let (_, capability_source) = SOURCES
        .iter()
        .find(|(file, _)| *file == "capability.rs")
        .expect("capability.rs is scanned");

    let widened = capability_source.replace("pub(crate) fn from_parts", "pub fn from_parts");
    assert_ne!(
        widened.as_str(), *capability_source,
        "`from_parts` is no longer declared the way this scan reads it, so the mutation below is a \
         no-op and this test proves nothing. Re-derive the mutation from the real declaration"
    );

    let widened_routes = routes_to_a_capability("capability.rs", &widened);
    assert!(
        widened_routes.contains(&("capability.rs", "from_parts".to_owned(), "pub".to_owned())),
        "the scan did not see a public `from_parts` after it was made public: {widened_routes:?}"
    );
    assert_ne!(
        widened_routes,
        routes_to_a_capability("capability.rs", capability_source),
        "the scan cannot tell the widened source from the real one"
    );
}

/// A module the scan does not read is a module the property does not cover, and adding one is
/// exactly how that happens.
#[test]
fn every_module_of_the_crate_is_scanned() {
    let lib = include_str!("../src/lib.rs");
    for line in lib.lines() {
        let Some(rest) = line.trim().strip_prefix("mod ") else {
            continue;
        };
        let Some(name) = rest.strip_suffix(';') else {
            continue;
        };
        let file = format!("{name}.rs");
        assert!(
            SOURCES.iter().any(|(listed, _)| *listed == file),
            "{file} is a module of `mesh-crypto` and this scan does not read it, so a route to a \
             `Capability` declared there would not be inventoried"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 3. The signature is what does the separating.
// ---------------------------------------------------------------------------------------------

/// Identical field values, identical bytes on the wire, one signature that checks under the key the
/// verifier trusts and one that does not. Only one of the two is ever a `Capability`.
///
/// This is the property the fence protects, stated as behaviour: authority is not in the bytes, it
/// is in the signature over them. A public `from_parts` would let a caller skip straight to the
/// second outcome with the first one's bytes.
#[test]
fn hostile_field_values_become_a_capability_only_when_the_signature_checks() {
    let human = TestCustody::new(0x11);
    let agent = TestCustody::new(0x22);

    let parts = hostile_parts(human.public_key(), agent.public_key());
    let payload = <PlumbingCodec as CapabilityCodec>::encode(&parts);

    // The attack: the agent writes field values claiming the human issued it publication authority,
    // and signs them with the only key it has — its own.
    let forged = CapabilityToken::new(
        agent
            .sign(&CapabilityToken::payload_to_sign(&payload))
            .expect("an agent can sign with its own key"),
        payload.clone(),
    )
    .expect("small payload");

    assert_eq!(
        forged.verify::<PlumbingScheme, PlumbingCodec, HumanHeld>(
            &human.public_key(),
            &agent.public_key(),
            NOW,
            EPOCH,
        ),
        Err(TokenError::Signature(VerifyError::Mismatch)),
        "field values naming publication authority became a capability without a signature the \
         verifier trusts"
    );

    // The sanctioned route: the same bytes, signed by the key the verifier trusts.
    let issued = CapabilityToken::new(
        human
            .sign(&CapabilityToken::payload_to_sign(&payload))
            .expect("the human's custody signs"),
        payload,
    )
    .expect("small payload");

    let capability = issued
        .verify::<PlumbingScheme, PlumbingCodec, HumanHeld>(
            &human.public_key(),
            &agent.public_key(),
            NOW,
            EPOCH,
        )
        .expect("a token signed by the trusted issuer verifies");

    assert!(
        capability.advances_canonical_head(),
        "the bytes named publication authority and the signature checked, so the capability carries it"
    );
}
