//! Harness entry points backed by the same native key custody as desktop private authoring.
//! These modes run before AppKit/Tauri setup and never switch the selected desktop workspace.

use mesh_crypto::{KeyCustody as _, SigningPayload};
use mesh_daemon::ipc::Json;
use mesh_daemon::project_attachment::{
    AttachmentCaptureService, CapturePhase, CaptureSchedule, ObservationLimits, ProjectAttachment,
};
use mesh_daemon::CheckpointSigner;
use mesh_keychain::SoftwareActorCustody;
use mesh_types::{PublicKey, Signature};
use std::io::{self, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// This key attests to a local capture session, not the author of externally edited files.
/// It stays in native custody, is never serialized, and has no human approval credential.
pub(crate) struct NativeCaptureSigner(SoftwareActorCustody);
impl NativeCaptureSigner {
    pub(crate) fn generate() -> Result<Arc<dyn CheckpointSigner>, String> {
        SoftwareActorCustody::generate()
            .map(|key| Arc::new(Self(key)) as Arc<dyn CheckpointSigner>)
            .map_err(|_| "The local capture identity could not be created".to_owned())
    }
}
impl CheckpointSigner for NativeCaptureSigner {
    fn public_key(&self) -> PublicKey {
        self.0.public_key().public_key()
    }
    fn sign(&self, payload: &SigningPayload) -> Result<Signature, String> {
        self.0
            .sign(payload)
            .map_err(|_| "The local capture identity could not sign".to_owned())
    }
}

/// A new import gets an ephemeral actor. Retried imports use only verified public proof.
/// No import private key is serialized or restored, and neither form can approve main.
pub(crate) struct NativeImportSigner(ImportIdentity);
enum ImportIdentity {
    Fresh(Box<SoftwareActorCustody>),
    Recorded(PublicKey),
}
impl NativeImportSigner {
    pub(crate) fn fresh() -> Result<Self, String> {
        SoftwareActorCustody::generate()
            .map(|key| Self(ImportIdentity::Fresh(Box::new(key))))
            .map_err(|_| "The private import identity could not be created".into())
    }
    pub(crate) fn recorded(actor: PublicKey) -> Self {
        Self(ImportIdentity::Recorded(actor))
    }
}
impl CheckpointSigner for NativeImportSigner {
    fn public_key(&self) -> PublicKey {
        match &self.0 {
            ImportIdentity::Fresh(key) => key.public_key().public_key(),
            ImportIdentity::Recorded(actor) => *actor,
        }
    }
    fn sign(&self, payload: &SigningPayload) -> Result<Signature, String> {
        match &self.0 {
            ImportIdentity::Fresh(key) => key
                .sign(payload)
                .map_err(|_| "The private import could not be signed".into()),
            ImportIdentity::Recorded(_) => {
                Err("Retained import identity cannot sign a new operation".into())
            }
        }
    }
}
impl mesh_daemon::fleet::CandidateImportSigner for NativeImportSigner {
    fn sign_import_provenance(&self, payload: &SigningPayload) -> Result<Signature, String> {
        self.sign(payload)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Capture,
    Watch,
    Versions,
}
#[derive(Debug, PartialEq, Eq)]
struct Invocation {
    action: Action,
    metadata: PathBuf,
}
fn parse(args: &[String]) -> Result<Option<Invocation>, String> {
    if args.first().map(String::as_str) != Some("--mesh-attachment") {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err(
            "Usage: Mesh --mesh-attachment <capture|watch|versions> <absolute-metadata-folder>"
                .to_owned(),
        );
    }
    let action = match args[1].as_str() {
        "capture" => Action::Capture,
        "watch" => Action::Watch,
        "versions" => Action::Versions,
        _ => return Err("Attachment action must be capture, watch or versions".to_owned()),
    };
    let metadata = PathBuf::from(&args[2]);
    if !metadata.is_absolute() {
        return Err("The attachment metadata folder must be absolute".to_owned());
    }
    Ok(Some(Invocation { action, metadata }))
}

pub fn run_if_requested() -> Option<Result<(), String>> {
    let invocation = match parse(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(Some(invocation)) => invocation,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    Some(run(invocation, io::stdin(), io::stdout()))
}
fn emit(output: &mut impl Write, value: Json) -> Result<(), String> {
    writeln!(output, "{}", value.encode())
        .and_then(|()| output.flush())
        .map_err(|_| "The attachment output stream closed".to_owned())
}
fn run(
    invocation: Invocation,
    input: impl Read + Send + 'static,
    mut output: impl Write,
) -> Result<(), String> {
    let attachment = ProjectAttachment::reopen(&invocation.metadata)
        .map_err(|_| "The registered project is unavailable or its identity changed".to_owned())?;
    match invocation.action {
        Action::Capture => {
            let signer = NativeCaptureSigner::generate()?;
            let captured = attachment
                .capture_inputs(ObservationLimits::default())
                .map_err(|_| {
                    "A complete bounded capture could not be read; no version was saved".to_owned()
                })?;
            let saved = attachment.save_capture(&invocation.metadata, &captured, signer.public_key(), |payload| signer.sign(payload))
                .map_err(|_| "Capture could not be confirmed; existing history was preserved and may need reconciliation".to_owned())?;
            emit(
                &mut output,
                Json::object([
                    ("schema", Json::text("mesh.attachment-save/v1")),
                    ("saved_version", Json::text(saved.operation().to_string())),
                    ("attribution", Json::text("unknown")),
                    ("atomic_snapshot", Json::Bool(false)),
                ]),
            )
        }
        Action::Versions => {
            let versions = attachment
                .saved_versions(&invocation.metadata)
                .map_err(|_| {
                    "Saved attachment history is unavailable or needs reconciliation".to_owned()
                })?;
            emit(
                &mut output,
                Json::object([
                    ("schema", Json::text("mesh.attachment-versions/v1")),
                    (
                        "versions",
                        Json::Array(
                            versions
                                .into_iter()
                                .map(|v| Json::text(v.operation().to_string()))
                                .collect(),
                        ),
                    ),
                ]),
            )
        }
        Action::Watch => {
            let service = AttachmentCaptureService::start(
                &invocation.metadata,
                NativeCaptureSigner::generate()?,
                CaptureSchedule::default(),
            )
            .map_err(|_| "Background capture could not start for this registration".to_owned())?;
            watch(service, input, &mut output)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Control {
    Capture,
    Status,
    Stop,
}
fn read_control(input: &mut impl Read) -> Result<Control, String> {
    let mut command = Vec::new();
    loop {
        let mut byte = [0];
        match input.read(&mut byte) {
            Ok(0) if command.is_empty() => return Ok(Control::Stop),
            Ok(0) => return Err("Incomplete attachment control command".to_owned()),
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) if command.len() < 32 => command.push(byte[0]),
            Ok(_) => return Err("Attachment control command is too long".to_owned()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err("The attachment control stream closed unexpectedly".to_owned()),
        }
    }
    if command.last() == Some(&b'\r') {
        command.pop();
    }
    match command.as_slice() {
        b"capture" => Ok(Control::Capture),
        b"status" => Ok(Control::Status),
        b"stop" => Ok(Control::Stop),
        _ => Err("Attachment control command must be capture, status or stop".to_owned()),
    }
}
fn watch(
    service: AttachmentCaptureService,
    input: impl Read + Send + 'static,
    output: &mut impl Write,
) -> Result<(), String> {
    let (sender, controls) = mpsc::sync_channel(1);
    let reader = std::thread::Builder::new()
        .name("mesh-attachment-control".to_owned())
        .spawn(move || {
            let mut input = BufReader::new(input);
            loop {
                let command = read_control(&mut input);
                let terminal = matches!(command, Ok(Control::Stop) | Err(_));
                if sender.send(command).is_err() || terminal {
                    break;
                }
            }
        });
    if reader.is_err() {
        service
            .stop_and_join()
            .map_err(|_| "Capture termination could not be confirmed".to_owned())?;
        return Err("The attachment control reader could not start".to_owned());
    }
    // The control reader owns no capture authority. A broken output can leave it waiting on stdin
    // until this dedicated headless process exits; the capture worker is always joined below.
    let result = (|| {
        let mut status = service.status();
        emit(output, status.to_json())?;
        loop {
            match controls.try_recv() {
                Ok(Ok(Control::Stop)) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                Ok(Err(problem)) => return Err(problem),
                Ok(Ok(Control::Capture)) => {
                    service.request_capture();
                }
                Ok(Ok(Control::Status)) => emit(output, service.status().to_json())?,
                Err(mpsc::TryRecvError::Empty) => {}
            }
            let next = service.wait_for_update(status.revision, Duration::from_millis(100));
            if next.revision != status.revision {
                emit(output, next.to_json())?;
            }
            if next.phase == CapturePhase::Failed {
                return Err("The attachment capture worker stopped unexpectedly".to_owned());
            }
            status = next;
        }
    })();
    let stopped = service
        .stop_and_join()
        .map_err(|_| "Capture termination could not be confirmed".to_owned())?;
    result?;
    if stopped.phase != CapturePhase::Stopped {
        return Err("The attachment capture worker did not stop cleanly".to_owned());
    }
    emit(output, stopped.to_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;

    #[test]
    fn imported_public_identity_recovers_without_any_signing_capability() {
        use mesh_daemon::fleet::CandidateImportSigner;
        let first = NativeImportSigner::fresh().unwrap();
        let second = NativeImportSigner::fresh().unwrap();
        assert_ne!(first.public_key(), second.public_key());
        let recovered = NativeImportSigner::recorded(first.public_key());
        let payload = SigningPayload::new(
            mesh_crypto::DomainSeparator::new("mesh.v0.fleet-project-import"),
            b"test receipt",
        );
        assert_eq!(recovered.public_key(), first.public_key());
        assert!(recovered.sign(&payload).is_err());
        assert!(recovered.sign_import_provenance(&payload).is_err());
        assert!(first.sign_import_provenance(&payload).is_ok());
    }

    #[test]
    fn invocation_is_explicit_and_refuses_ambiguous_arguments() {
        let args = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
        assert!(parse(&args(&["ordinary"])).unwrap().is_none());
        assert_eq!(
            parse(&args(&["--mesh-attachment", "capture", "/metadata"]))
                .unwrap()
                .unwrap()
                .action,
            Action::Capture
        );
        for values in [
            vec!["--mesh-attachment"],
            vec!["--mesh-attachment", "capture", "relative"],
            vec!["--mesh-attachment", "approve", "/metadata"],
            vec!["--mesh-attachment", "capture", "/metadata", "extra"],
        ] {
            assert!(parse(&args(&values)).is_err());
        }
    }

    #[test]
    fn native_capture_sessions_save_and_list_without_persisting_a_key() {
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-attachment-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("project");
        let metadata = root.join("metadata");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&metadata).unwrap();
        fs::write(source.join("work"), b"one").unwrap();
        let attached = ProjectAttachment::register(&source, &metadata).unwrap();
        let mut output = Vec::new();
        run(
            Invocation {
                action: Action::Capture,
                metadata: metadata.clone(),
            },
            Cursor::new(Vec::<u8>::new()),
            &mut output,
        )
        .unwrap();
        let first = attached.saved_versions(&metadata).unwrap()[0];
        fs::write(source.join("work"), b"two").unwrap();
        run(
            Invocation {
                action: Action::Capture,
                metadata: metadata.clone(),
            },
            Cursor::new(Vec::<u8>::new()),
            &mut output,
        )
        .unwrap();
        let versions = attached.saved_versions(&metadata).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(
            attached
                .saved_file(&metadata, first, "work")
                .unwrap()
                .unwrap(),
            b"one"
        );
        let before = fs::read(metadata.join(mesh_daemon::RECORD_FILE_NAME)).unwrap();
        output.clear();
        run(
            Invocation {
                action: Action::Versions,
                metadata: metadata.clone(),
            },
            Cursor::new(Vec::<u8>::new()),
            &mut output,
        )
        .unwrap();
        let listed = Json::parse(std::str::from_utf8(&output).unwrap().trim()).unwrap();
        assert_eq!(
            listed.get("versions"),
            Some(&Json::Array(
                versions
                    .iter()
                    .map(|v| Json::text(v.operation().to_string()))
                    .collect()
            ))
        );
        assert_eq!(
            fs::read(metadata.join(mesh_daemon::RECORD_FILE_NAME)).unwrap(),
            before
        );
        assert_eq!(fs::read_dir(&source).unwrap().count(), 1);
        assert!(fs::read_dir(&metadata).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("key")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn control_input_is_bounded_and_never_echoes_untrusted_input() {
        for (input, expected) in [
            (b"capture\n".as_slice(), Control::Capture),
            (b"status\r\n".as_slice(), Control::Status),
            (b"stop\n".as_slice(), Control::Stop),
            (b"".as_slice(), Control::Stop),
        ] {
            assert_eq!(read_control(&mut Cursor::new(input)).unwrap(), expected);
        }
        assert!(read_control(&mut Cursor::new(b"capture")).is_err());
        let failure = read_control(&mut Cursor::new(vec![b'x'; 4096])).unwrap_err();
        assert!(!failure.contains("xxx"));
    }
}
