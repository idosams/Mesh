//! Actual preparation and private staging after authenticated native publication.
use super::*;
use crate::project_attachment::{
    NativeConsumedStartRequest, NativeGrantInspection, NativeTrustedConsumedStartRequest,
    NativeTrustedInputGrantRequest, PreparedNativeTrustedConsumedStart, SavedAttachmentVersion,
};
use ed25519_dalek::{Signer as _, SigningKey};
use std::{
    collections::BTreeMap,
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const ENV: &str = "MESH_NATIVE_CONSUMPTION_STAGE_RESTART";
const CHILD: &str = "project_attachment::dependency_publication_replay::tests::native_consumption_start::consumption_stage_restart_child";
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
fn signer(payload: &SigningPayload) -> Result<Signature, &'static str> {
    Ok(Signature::from_bytes(
        SigningKey::from_bytes(&[238; 32])
            .sign(payload.as_bytes())
            .to_bytes(),
    ))
}
fn actor() -> PublicKey {
    PublicKey::from_bytes(
        SigningKey::from_bytes(&[238; 32])
            .verifying_key()
            .to_bytes(),
    )
}
#[allow(clippy::too_many_arguments)]
fn prepare(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    operation: RecordDigest,
    destination: &ProvisionedAttachment,
    grant: RecordDigest,
    request: RecordDigest,
    trusted: &crate::TrustedReviewers,
) -> std::io::Result<PreparedNativeTrustedConsumedStart> {
    storage.prepare_native_consumed_start(
        owner,
        NativeTrustedConsumedStartRequest {
            source,
            operation,
            destination,
            grant,
            available: &[owner],
            request,
            limits: ObservationLimits::default(),
        },
        actor(),
        trusted,
        signer,
    )
}
// Synthetic fixture bytes only. Include modes and physical identities so refusal cannot repair
// or substitute existing evidence. No production cleanup or recovery is implemented here.
type Tree = BTreeMap<PathBuf, (u64, u64, u32, Vec<u8>)>;
fn tree(path: &Path) -> Tree {
    fn walk(base: &Path, path: &Path, out: &mut Tree) {
        let meta = fs::symlink_metadata(path).unwrap();
        assert!(!meta.file_type().is_symlink());
        out.insert(
            path.strip_prefix(base).unwrap().to_path_buf(),
            (
                meta.dev(),
                meta.ino(),
                meta.mode(),
                if meta.is_file() {
                    fs::read(path).unwrap()
                } else {
                    vec![]
                },
            ),
        );
        if meta.is_dir() {
            for child in fs::read_dir(path).unwrap() {
                walk(base, &child.unwrap().path(), out);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(path, path, &mut out);
    out
}
#[test]
fn consumption_stage_restart_child() {
    let Ok(raw) = std::env::var(ENV) else {
        return;
    };
    let value = Json::parse(&raw).unwrap();
    let text = |key| value.get(key).and_then(Json::as_text).unwrap();
    let root = Path::new(text("root"));
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let owner = storage.reopen(text("owner")).unwrap();
    let source = storage.reopen(text("source")).unwrap();
    let destination = storage.reopen(text("destination")).unwrap();
    let public = fs::read(root.join("consumption-stage-public-key")).unwrap();
    let trusted = if text("mode") == "missing-trust" {
        crate::TrustedReviewers::default()
    } else {
        crate::TrustedReviewers::with_human_credentials([HumanApprovalCredential::from_public_key(
            public.as_slice().try_into().unwrap(),
        )
        .unwrap()])
    };
    let operation = if text("mode") == "foreign-operation" {
        id(250)
    } else {
        RecordDigest::parse_hex(text("operation")).unwrap()
    };
    let result = prepare(
        &storage,
        &owner,
        &source,
        operation,
        &destination,
        RecordDigest::parse_hex(text("grant")).unwrap(),
        RecordDigest::parse_hex(text("request")).unwrap(),
        &trusted,
    )
    .and_then(|prepared| {
        prepared
            .stage_with_hook(&storage, |phase, _| {
                if phase == text("mode") {
                    std::process::exit(86);
                }
                Ok(())
            })
            .map(|staged| {
                Json::object([
                    ("operation", Json::text(prepared.operation().to_hex())),
                    ("receipt", Json::text(staged.receipt().unwrap().to_hex())),
                ])
            })
    });
    if matches!(
        text("mode"),
        "missing-trust" | "foreign-operation" | "refused"
    ) {
        assert!(result.is_err());
    } else {
        fs::write(
            root.join("consumption-stage-result"),
            result.unwrap().encode(),
        )
        .unwrap();
    }
}
fn run_child(root: &Path, mut value: Json, mode: &str, expected: i32) {
    let Json::Object(fields) = &mut value else {
        panic!("invalid stage fixture");
    };
    fields.push(("mode".into(), Json::text(mode)));
    let log = root.join(format!("consumption-stage-{mode}.log"));
    let output = fs::File::create(&log).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD, "--nocapture"])
        .env(ENV, value.encode())
        .stdout(Stdio::from(output.try_clone().unwrap()))
        .stderr(Stdio::from(output))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "native stage {mode} timeout: {}",
                fs::read_to_string(&log).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(
        status.code(),
        Some(expected),
        "{}",
        fs::read_to_string(log).unwrap()
    );
}
pub(super) fn inspect(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    source: &ProvisionedAttachment,
    version: SavedAttachmentVersion,
    trusted: &crate::TrustedReviewers,
    public: &[u8],
    saved_note: &[u8],
) {
    let destination = storage
        .reserve_native_dependency_lane(
            owner,
            source,
            version.operation(),
            id(239),
            &[owner],
            trusted,
        )
        .unwrap();
    let grant = storage
        .grant_native_saved_input(
            owner,
            NativeTrustedInputGrantRequest {
                source,
                operation: version.operation(),
                destination: &destination,
                allowed: true,
                expected_previous: None,
                request: id(240),
            },
            &[owner],
            trusted,
        )
        .unwrap()
        .record();
    let operation = version.operation();
    let root = owner.project().root().parent().unwrap();
    let allocation = destination.project().root().parent().unwrap();
    let foreign_path = root.join("foreign-consumption-project");
    fs::create_dir(&foreign_path).unwrap();
    fs::write(
        foreign_path.join("foreign-editor"),
        b"another project remains independent",
    )
    .unwrap();
    let foreign = storage.provision(&foreign_path).unwrap();
    let foreign_tree = tree(&foreign_path);
    let source_note = source.project().root().join("note");
    fs::write(
        &source_note,
        b"newer live editor bytes must remain untouched",
    )
    .unwrap();
    let live_ignore = source.project().root().join(".gitignore");
    fs::write(&live_ignore, b"note\n").unwrap();
    let git = owner.project().root().join(".git");
    fs::create_dir(&git).unwrap();
    fs::write(git.join("HEAD"), b"ref: refs/heads/user-consumption\n").unwrap();
    fs::write(git.join("index"), b"synthetic user staging").unwrap();
    let editor_tree = tree(owner.project().root());
    let source_tree = tree(source.project().root());
    let journals = [owner, source, &destination].map(|work| {
        let path = work.metadata_path().join(crate::RECORD_FILE_NAME);
        let bytes = fs::read(&path).unwrap();
        (path, bytes)
    });
    let before = tree(allocation);
    let count = storage.registrations().unwrap().len();
    let preserve = || {
        assert_eq!(tree(owner.project().root()), editor_tree);
        assert_eq!(tree(source.project().root()), source_tree);
        for (p, bytes) in &journals {
            assert_eq!(fs::read(p).unwrap(), *bytes);
        }
        assert_eq!(storage.registrations().unwrap().len(), count);
        assert_eq!(
            fs::read_dir(destination.project().root()).unwrap().count(),
            0
        );
        assert!(destination.saved_versions().unwrap().is_empty());
    };
    // The old path still refuses the same real publication and typed saved token.
    assert!(storage
        .prepare_consumed_start(
            owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source,
                    version,
                    destination: &destination,
                    grant
                },
                available: &[owner],
                request: id(241),
                limits: ObservationLimits::default(),
            },
            actor(),
            signer
        )
        .is_err());
    for (selected_operation, selected_grant, request, trust) in [
        (id(0), grant, id(241), trusted),
        (id(250), grant, id(241), trusted),
        (operation, id(0), id(241), trusted),
        (operation, id(250), id(241), trusted),
        (operation, grant, id(0), trusted),
        (operation, grant, id(240), trusted),
        (
            operation,
            grant,
            id(241),
            &crate::TrustedReviewers::default(),
        ),
    ] {
        assert!(prepare(
            storage,
            owner,
            source,
            selected_operation,
            &destination,
            selected_grant,
            request,
            trust
        )
        .is_err());
    }
    for available in [vec![owner; 257], vec![&destination], vec![&foreign]] {
        assert!(storage
            .prepare_native_consumed_start(
                owner,
                NativeTrustedConsumedStartRequest {
                    source,
                    operation,
                    destination: &destination,
                    grant,
                    available: &available,
                    request: id(241),
                    limits: ObservationLimits::default(),
                },
                actor(),
                trusted,
                signer
            )
            .is_err());
    }
    assert!(storage
        .prepare_native_consumed_start(
            owner,
            NativeTrustedConsumedStartRequest {
                source,
                operation,
                destination: &destination,
                grant,
                available: &[owner],
                request: id(241),
                limits: ObservationLimits {
                    entries: 0,
                    ..ObservationLimits::default()
                },
            },
            actor(),
            trusted,
            signer
        )
        .is_err());
    assert!(storage
        .prepare_native_consumed_start(
            owner,
            NativeTrustedConsumedStartRequest {
                source,
                operation,
                destination: &destination,
                grant,
                available: &[owner],
                request: id(241),
                limits: ObservationLimits {
                    bytes: 1,
                    file_bytes: 1,
                    ..ObservationLimits::default()
                },
            },
            actor(),
            trusted,
            signer
        )
        .is_err());
    for (selected_owner, selected_source, selected_destination) in [
        (&foreign, source, &destination),
        (owner, &foreign, &destination),
        (owner, source, &foreign),
        (owner, &destination, &destination),
        (owner, source, owner),
    ] {
        assert!(prepare(
            storage,
            selected_owner,
            selected_source,
            operation,
            selected_destination,
            grant,
            id(241),
            trusted
        )
        .is_err());
    }
    assert_eq!(tree(&foreign_path), foreign_tree);
    assert_eq!(tree(allocation), before);
    preserve();
    let prepared = prepare(
        storage,
        owner,
        source,
        operation,
        &destination,
        grant,
        id(241),
        trusted,
    )
    .unwrap();
    assert_eq!(prepared.request(), id(241));
    assert_ne!(prepared.operation(), id(0));
    assert_ne!(prepared.dependency_digest(), id(0));
    for pending in [
        super::super::super::dependency_decision::PENDING,
        super::super::super::history::dependency_capture::PENDING,
        super::super::super::dependency_private_context::publication::PENDING,
    ] {
        let p = source.store.stable_namespace().unwrap().0.join(pending);
        // Fixture-owned injection; production must retain every ambiguous byte.
        fs::write(&p, b"incomplete native source transaction").unwrap();
        assert!(prepared.stage(storage).is_err());
        assert_eq!(
            fs::read(&p).unwrap(),
            b"incomplete native source transaction"
        );
        fs::remove_file(p).unwrap();
        assert_eq!(tree(allocation), before);
    }
    preserve();
    let mut published_path = None;
    let staged = prepared
        .stage_with_hook(storage, |phase, pin| {
            if phase == "published" {
                published_path = Some(pin.stable_namespace()?.0);
            }
            Ok(())
        })
        .unwrap();
    let stage_path = published_path.unwrap();
    assert_eq!(
        fs::read(stage_path.join("entry-0/exchange")).unwrap(),
        saved_note
    );
    let receipt = staged.receipt().unwrap();
    assert_ne!(receipt, id(0));
    assert_eq!(staged.request(), id(241));
    let stable = tree(allocation);
    assert_eq!(prepared.stage(storage).unwrap().receipt().unwrap(), receipt);
    assert_eq!(tree(allocation), stable);
    preserve();
    // Changes after the complete custody phase must still refuse acknowledgement.
    let frames_path = stage_path.join("frames.mesh");
    let original_frames = fs::read(&frames_path).unwrap();
    assert!(prepared
        .stage_with_hook(storage, |phase, _| {
            if phase == "custody-released" {
                fs::write(&frames_path, b"changed after custody release")?;
            }
            Ok(())
        })
        .is_err());
    assert_eq!(
        fs::read(&frames_path).unwrap(),
        b"changed after custody release"
    );
    fs::write(&frames_path, original_frames).unwrap();
    let unexpected = destination.project().root().join("unexpected-editor-work");
    assert!(prepared
        .stage_with_hook(storage, |phase, _| {
            if phase == "custody-released" {
                fs::write(&unexpected, b"preserve late editor work")?;
            }
            Ok(())
        })
        .is_err());
    assert_eq!(fs::read(&unexpected).unwrap(), b"preserve late editor work");
    fs::rename(
        &unexpected,
        root.join("retained-late-consumption-editor-work"),
    )
    .unwrap();
    assert_eq!(prepared.stage(storage).unwrap().receipt().unwrap(), receipt);
    for name in ["frames.mesh", "stage.json"] {
        let p = stage_path.join(name);
        let original = fs::read(&p).unwrap();
        fs::write(&p, b"foreign stage evidence").unwrap();
        let refused = tree(allocation);
        assert!(prepared.stage(storage).is_err());
        assert_eq!(tree(allocation), refused);
        fs::write(p, original).unwrap();
    }
    let outer = stage_path.join("stage.json");
    fs::set_permissions(&outer, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(prepared.stage(storage).is_err());
    assert_eq!(fs::metadata(&outer).unwrap().mode() & 0o777, 0o644);
    fs::set_permissions(&outer, fs::Permissions::from_mode(0o600)).unwrap();
    let link = root.join("retained-native-stage-link");
    fs::hard_link(&outer, &link).unwrap();
    assert!(prepared.stage(storage).is_err());
    assert_eq!(fs::metadata(&outer).unwrap().nlink(), 2);
    fs::remove_file(link).unwrap();
    assert_eq!(prepared.stage(storage).unwrap().receipt().unwrap(), receipt);
    preserve();
    fs::write(root.join("consumption-stage-public-key"), public).unwrap();
    // Real process exits at every new writer boundary; pre-publication attempts are retained.
    for (index, phase) in [
        "attempt-created",
        "entries-staged",
        "bundle-synced",
        "published",
        "custody-released",
    ]
    .into_iter()
    .enumerate()
    {
        let request = id(242 + index as u8);
        let value = Json::object([
            ("root", Json::text(root.to_string_lossy())),
            ("owner", Json::text(owner.id())),
            ("source", Json::text(source.id())),
            ("destination", Json::text(destination.id())),
            ("operation", Json::text(operation.to_hex())),
            ("grant", Json::text(grant.to_hex())),
            ("request", Json::text(request.to_hex())),
        ]);
        run_child(root, value.clone(), phase, 86);
        let interrupted = tree(allocation);
        for mode in ["missing-trust", "foreign-operation"] {
            run_child(root, value.clone(), mode, 0);
            assert_eq!(tree(allocation), interrupted);
        }
        let target = allocation.join(format!("consumption-{}", request.to_hex()));
        let published = target.exists().then(|| tree(&target));
        run_child(root, value.clone(), "recover", 0);
        if let Some(published) = published {
            assert_eq!(tree(&target), published);
        }
        for (path, state) in &interrupted {
            assert_eq!(tree(allocation).get(path), Some(state));
        }
        let recovered = tree(allocation);
        let result = fs::read(root.join("consumption-stage-result")).unwrap();
        run_child(root, value, "retry", 0);
        assert_eq!(
            fs::read(root.join("consumption-stage-result")).unwrap(),
            result
        );
        assert_eq!(tree(allocation), recovered);
        preserve();
    }
    // Source substitution after private publication refuses acknowledgment and preserves both trees.
    let source_path = source.project().root().to_path_buf();
    let retained = source_path.with_extension("consumption-original");
    let substituted = prepare(
        storage,
        owner,
        source,
        operation,
        &destination,
        grant,
        id(247),
        trusted,
    )
    .unwrap();
    assert!(substituted
        .stage_with_hook(storage, |phase, _| {
            if phase == "published" {
                fs::rename(&source_path, &retained)?;
                fs::create_dir(&source_path)?;
                fs::write(
                    source_path.join("foreign-editor"),
                    b"preserve substituted user work",
                )?;
            }
            Ok(())
        })
        .is_err());
    assert_eq!(
        fs::read(source_path.join("foreign-editor")).unwrap(),
        b"preserve substituted user work"
    );
    fs::rename(
        &source_path,
        source_path.with_extension("consumption-foreign"),
    )
    .unwrap();
    fs::rename(&retained, &source_path).unwrap();
    substituted.stage(storage).unwrap();
    preserve();
    // Unrelated valid owner records between phases do not invalidate this exact current input.
    let other = storage
        .reserve_native_dependency_lane(owner, source, operation, id(253), &[owner], trusted)
        .unwrap();
    storage
        .grant_native_saved_input(
            owner,
            NativeTrustedInputGrantRequest {
                source,
                operation,
                destination: &other,
                allowed: true,
                expected_previous: None,
                request: id(254),
            },
            &[owner],
            trusted,
        )
        .unwrap();
    prepared.revalidate(storage).unwrap();
    assert_eq!(prepared.stage(storage).unwrap().receipt().unwrap(), receipt);
    assert_eq!(fs::read(&journals[2].0).unwrap(), journals[2].1);
    assert_eq!(tree(&foreign_path), foreign_tree);
    // A protected external callback may perform a real native grant write: no custody is held.
    // Its revocation must be observed before any prepared candidate is returned.
    let prior_stage = tree(allocation);
    let mut revocation = None;
    assert!(storage
        .prepare_native_consumed_start(
            owner,
            NativeTrustedConsumedStartRequest {
                source,
                operation,
                destination: &destination,
                grant,
                available: &[owner],
                request: id(248),
                limits: ObservationLimits::default(),
            },
            actor(),
            trusted,
            |payload| {
                revocation = Some(
                    storage
                        .grant_native_saved_input(
                            owner,
                            NativeTrustedInputGrantRequest {
                                source,
                                operation,
                                destination: &destination,
                                allowed: false,
                                expected_previous: Some(grant),
                                request: id(249),
                            },
                            &[owner],
                            trusted,
                        )
                        .unwrap()
                        .record(),
                );
                signer(payload)
            }
        )
        .is_err());
    assert!(prepared.revalidate(storage).is_err());
    assert!(prepared.stage(storage).is_err());
    assert_eq!(tree(allocation), prior_stage);
    let new_grant = storage
        .grant_native_saved_input(
            owner,
            NativeTrustedInputGrantRequest {
                source,
                operation,
                destination: &destination,
                allowed: true,
                expected_previous: revocation,
                request: id(251),
            },
            &[owner],
            trusted,
        )
        .unwrap()
        .record();
    assert!(prepared.stage(storage).is_err()); // Regrant does not revive the exact old grant.
    prepare(
        storage,
        owner,
        source,
        operation,
        &destination,
        new_grant,
        id(252),
        trusted,
    )
    .unwrap()
    .stage(storage)
    .unwrap();
    assert_eq!(tree(owner.project().root()), editor_tree);
    assert_eq!(tree(source.project().root()), source_tree);
    assert_eq!(
        fs::read_dir(destination.project().root()).unwrap().count(),
        0
    );
    assert!(destination.saved_versions().unwrap().is_empty());
}
