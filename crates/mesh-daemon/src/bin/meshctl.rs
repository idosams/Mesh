//! `meshctl` — talk to a running Mesh background service from a terminal.
//!
//! # Why this exists
//!
//! A daemon nobody can call is a daemon nobody can check. This is the smallest client that proves
//! the surface answers: it connects to the socket `meshd` printed, negotiates a version, makes one
//! call, prints the reply as one line of JSON, and exits on whether the daemon answered.
//!
//! ```text
//! meshctl --endpoint /run/user/1000/mesh/daemon.sock status
//! meshctl --endpoint … open ./my-workspace
//! meshctl --endpoint … state
//! meshctl --endpoint … watch 2
//! ```
//!
//! It is deliberately not the desktop application, and it is deliberately not a second
//! implementation of the wire: every message it sends and reads goes through
//! `mesh_daemon::ipc::message`, so a change to the format cannot pass here and fail there.
//!
//! # Exit codes
//!
//! `0` the daemon answered · `1` the daemon refused or failed · `2` the client could not reach it
//! or the command line was wrong. Read them directly; a refusal is not a crash and a crash is not
//! a refusal.

use std::process::ExitCode;

#[cfg(not(unix))]
fn main() -> ExitCode {
    eprintln!(
        "meshctl: this build has no local transport for this platform and nothing was sent. \
         The Mesh background service listens on a Unix-domain socket, and the named-pipe backend \
         is not implemented."
    );
    ExitCode::from(2)
}

#[cfg(unix)]
fn main() -> ExitCode {
    match call::run(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(problem) => {
            eprintln!("meshctl: {problem}");
            eprintln!("meshctl: nothing was changed. `meshctl --help` lists the commands.");
            ExitCode::from(2)
        }
    }
}

#[cfg(unix)]
mod call {
    use std::fs::{self, File};
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::os::unix::fs::MetadataExt as _;
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::process::ExitCode;
    use std::time::Duration;

    use ed25519_dalek::SigningKey;
    use mesh_approval::{Digest32 as ApprovalDigest32, HeadId, ReviewBundleId};
    use mesh_daemon::ipc::{
        ChunkAssembler, ClientMessage, DaemonMessage, Json, SUPPORTED_VERSIONS,
    };
    use mesh_operations::{ObjectId, VersionId};
    use mesh_types::{Blake3, Digest32, PublicKey};

    /// How long to wait for one reply before giving up.
    ///
    /// Every method on this surface answers from memory or from one pass over a local file, so a
    /// wait this long means the daemon is wedged rather than busy, and saying so beats hanging.
    const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
    const UNSAFE_SUPPORT_REPLY: &str =
        "the service did not return a safe support preview; nothing was printed";

    /// Run one command.
    ///
    /// # Errors
    ///
    /// A sentence naming what could not be done. Every one of them means nothing was sent.
    pub fn run(arguments: Vec<String>) -> Result<ExitCode, String> {
        let mut request = Request::parse(&arguments)?;
        if request.endpoint.is_some() {
            if let Some(Local::SupportBundle { workspace }) = request.local.take() {
                let target = mesh_daemon::SupportBundle::pin_live_target(Path::new(&workspace))
                    .map_err(|error| {
                        format!("could not pin the requested workspace journal: {error}")
                    })?;
                request.plan = Some(Plan {
                    method: "workspace.state",
                    params: Json::empty_object(),
                    follow: 0,
                    output: Output::SupportBundle { workspace, target },
                });
            }
        }
        if let Some(local) = request.local {
            return local.run();
        }
        let Some(plan) = request.plan else {
            print!("{USAGE}");
            return Ok(ExitCode::SUCCESS);
        };
        let endpoint = request
            .endpoint
            .ok_or("`--endpoint` is required: pass the path `meshd` printed when it started")?;

        let stream = UnixStream::connect(&endpoint)
            .map_err(|error| format!("could not reach the service at {endpoint}: {error}"))?;
        stream
            .set_read_timeout(Some(REPLY_TIMEOUT))
            .map_err(|error| format!("could not set a reply timeout: {error}"))?;
        let mut writer = stream
            .try_clone()
            .map_err(|error| format!("could not use the connection: {error}"))?;
        let mut reader = BufReader::new(stream);

        send(
            &mut writer,
            &ClientMessage::Hello {
                id: 1,
                versions: SUPPORTED_VERSIONS.to_vec(),
                session: "meshctl".to_owned(),
            },
        )?;
        let support_output = matches!(&plan.output, Output::SupportBundle { .. });
        let mut assembler = ChunkAssembler::default();
        let welcome = read(&mut reader, &mut assembler).map_err(|problem| {
            if support_output {
                UNSAFE_SUPPORT_REPLY.to_owned()
            } else {
                problem
            }
        })?;
        let version = match &welcome {
            DaemonMessage::Welcome { version, .. } => *version,
            other => {
                if support_output {
                    return Err(UNSAFE_SUPPORT_REPLY.to_owned());
                }
                report(other);
                return Ok(ExitCode::FAILURE);
            }
        };
        if matches!(&plan.output, Output::Wire) {
            report(&welcome);
        }

        send(
            &mut writer,
            &ClientMessage::Call {
                id: 2,
                method: plan.method.to_owned(),
                version,
                params: plan.params,
            },
        )?;
        let reply = read(&mut reader, &mut assembler).map_err(|problem| {
            if support_output {
                UNSAFE_SUPPORT_REPLY.to_owned()
            } else {
                problem
            }
        })?;
        match &plan.output {
            Output::Wire => report(&reply),
            Output::SupportBundle { workspace, target } => {
                let DaemonMessage::Result { value, .. } = &reply else {
                    return Err(UNSAFE_SUPPORT_REPLY.to_owned());
                };
                let root = value
                    .get("root")
                    .and_then(Json::as_text)
                    .ok_or("the service returned workspace state without a root")?;
                if !same_workspace(Path::new(workspace), Path::new(root))? {
                    return Err("the service has a different workspace open".to_owned());
                }
                let bundle = value.get("support_bundle").ok_or(
                    "the service is too old to produce a live support bundle; stop it or upgrade it before using the endpoint-free fallback",
                )?;
                mesh_daemon::SupportBundle::validate_untrusted_preview(bundle)
                    .map_err(str::to_owned)?;
                let correlation = bundle
                    .get("workspace_correlation")
                    .and_then(Json::as_text)
                    .ok_or("the service returned a live support bundle without a correlation")?;
                if correlation != target.correlation() {
                    return Err(
                        "the service has a different workspace open; the requested journal generation does not match the support bundle"
                            .to_owned(),
                    );
                }
                println!("{}", bundle.encode());
            }
        }
        if !matches!(reply, DaemonMessage::Result { .. }) {
            return Ok(ExitCode::FAILURE);
        }

        // `watch` keeps reading, because the point of it is the lines nobody asked for.
        for _ in 0..plan.follow {
            match read(&mut reader, &mut assembler) {
                Ok(pushed) => report(&pushed),
                Err(problem) => {
                    eprintln!("meshctl: {problem}");
                    return Ok(ExitCode::FAILURE);
                }
            }
        }
        Ok(ExitCode::SUCCESS)
    }

    /// Write one message and flush it, so a reply cannot be waited on before the request left.
    fn send(writer: &mut UnixStream, message: &ClientMessage) -> Result<(), String> {
        writeln!(writer, "{}", message.encode())
            .and_then(|()| writer.flush())
            .map_err(|error| format!("could not send the request: {error}"))
    }

    /// Read one whole line and decode it.
    fn read(
        reader: &mut BufReader<UnixStream>,
        assembler: &mut ChunkAssembler,
    ) -> Result<DaemonMessage, String> {
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    return Err("the service closed the connection without answering".to_owned())
                }
                Ok(_) => {
                    let frame = DaemonMessage::decode(line.trim_end()).map_err(|error| {
                        format!("the service sent something unreadable: {error}")
                    })?;
                    if let Some(message) = assembler.push(frame).map_err(|error| {
                        format!("the service sent invalid response chunks: {error}")
                    })? {
                        return Ok(message);
                    }
                }
                Err(error) => return Err(format!("could not read the answer: {error}")),
            }
        }
    }

    /// Print one message as the exact line it was on the wire.
    ///
    /// Re-encoded rather than echoed, so what is printed is what this client *understood* — a line
    /// it mis-parsed prints differently from the one that arrived, which is the failure worth
    /// seeing.
    fn report(message: &DaemonMessage) {
        println!("{}", message.encode());
    }

    /// Which method to call, with what, and how many pushed lines to wait for afterwards.
    #[derive(Debug)]
    struct Plan {
        method: &'static str,
        params: Json,
        follow: usize,
        output: Output,
    }

    #[derive(Debug)]
    enum Output {
        Wire,
        SupportBundle {
            workspace: String,
            target: mesh_daemon::PinnedSupportTarget,
        },
    }

    /// A command answered here, without the background service.
    ///
    /// All are local for the same reason: they need nothing the service holds.
    /// The exclusion sources are files and `mesh_store::ExclusionSet::verdict` is a pure function;
    /// the backend choice is a total function of what is linked into this binary. Making a user
    /// start a daemon to be told why a file is not being saved — or what Mesh cannot see about a
    /// folder — would put the answer furthest away exactly when they most need it.
    #[derive(Debug)]
    enum Local {
        /// What this workspace does not version, and — for one path — which source said so.
        Exclusions {
            workspace: String,
            path: Option<String>,
        },
        /// Which mechanism Mesh will present a folder through, and what that mechanism misses.
        Restrictions,
        /// The exact redacted document a person can inspect before choosing to share it.
        SupportBundle { workspace: String },
        /// Hash an existing folder without writing it or creating a managed copy.
        ImportPreview { source: String },
        /// Register an existing project without moving it or claiming write custody.
        Attach { source: String, metadata: String },
        /// Verify an existing attachment after restart.
        AttachmentStatus { metadata: String, observe: bool },
        /// Copy and confirm the exact summary a person previously previewed.
        ImportConfirm {
            source: String,
            destination: String,
            expected: Digest32,
        },
        /// Reopen a durable import receipt and remove only an unchanged managed copy.
        ImportRollback { destination: String },
        /// A checked restore plan derived from local durable state, with no write authority.
        RestorePreview {
            workspace: String,
            object: ObjectId,
            target: VersionId,
        },
    }

    impl Local {
        fn run(self) -> Result<ExitCode, String> {
            match self {
                Self::Attach { source, metadata } => {
                    let result = mesh_daemon::project_attachment::ProjectAttachment::register(
                        Path::new(&source),
                        Path::new(&metadata),
                    )
                    .and_then(|attached| attached.status());
                    match result {
                        Ok(status) => println!("{}", status.encode()),
                        Err(error) => {
                            eprintln!("meshctl: attachment could not be confirmed: {error}. Existing files and any receipt were preserved.");
                            return Ok(ExitCode::from(1));
                        }
                    }
                }
                Self::AttachmentStatus { metadata, observe } => {
                    match mesh_daemon::project_attachment::ProjectAttachment::reopen(Path::new(
                        &metadata,
                    ))
                    .and_then(|attached| {
                        if observe {
                            attached.observe(
                                mesh_daemon::project_attachment::ObservationLimits::default(),
                            )
                        } else {
                            attached.status()
                        }
                    }) {
                        Ok(status) => println!("{}", status.encode()),
                        Err(error) => {
                            eprintln!("meshctl: attachment is unavailable: {error}");
                            return Ok(ExitCode::from(1));
                        }
                    }
                }
                Self::Exclusions { workspace, path } => {
                    let effective = mesh_daemon::EffectiveExclusions::load(
                        std::path::Path::new(&workspace),
                        None,
                    )
                    .map_err(|failure| format!("{failure}"))?;
                    println!("{}", effective.report(path.as_deref()).encode());
                }
                Self::Restrictions => {
                    // The same call `meshd` makes, so the two cannot disagree about what this
                    // build can reach. A person who runs this before starting the service is told
                    // what they are about to get rather than after.
                    let choice = mesh_daemon::choose_backend(mesh_daemon::Availability::probe());
                    println!("{}", choice.to_json().encode());
                }
                Self::SupportBundle { workspace } => {
                    let bundle = mesh_daemon::SupportBundle::collect(Path::new(&workspace));
                    println!("{}", bundle.preview());
                }
                Self::ImportPreview { source } => {
                    let summary = match mesh_daemon::preview_folder_import(Path::new(&source)) {
                        Ok(summary) => summary,
                        Err(failure) => return Ok(refused(failure)),
                    };
                    // The offline route can only describe an ordinary folder. Emit the same
                    // scope-bound token as a live daemon would for that mode, so a person can
                    // preview before starting Mesh and safely confirm through `--endpoint`
                    // afterward. Older clients emitted the raw content digest here, which looked
                    // interchangeable but produced a false "folder changed" refusal online.
                    println!("{}", summary.to_preview_json(false).encode());
                }
                Self::ImportConfirm {
                    source,
                    destination,
                    expected,
                } => {
                    let prepared = match mesh_daemon::PreparedFolderImport::prepare_presented(
                        Path::new(&source),
                        Path::new(&destination),
                    ) {
                        Ok(prepared) => prepared,
                        Err(failure) => return Ok(refused(failure)),
                    };
                    let found = prepared.summary().preview_confirmation_digest(false);
                    // Keep accepting the historical endpoint-free raw digest during the alpha
                    // transition. It is safe only here because this route always uses the
                    // ordinary-folder scanner and never the private-fenced open-workspace mode.
                    if found != expected && prepared.summary().digest() != expected {
                        let problem = format!(
                            "folder changed since preview: expected {expected}, found {}. The managed copy was rolled back",
                            found
                        );
                        if let Err(cleanup) = prepared.rollback() {
                            return Ok(refused(format!("{problem}; {cleanup}")));
                        }
                        return Ok(refused(problem));
                    }
                    let (confirmed, managed) = match prepared.confirm_into_workspace() {
                        Ok(confirmed) => confirmed,
                        Err(failure) => return Ok(refused(failure)),
                    };
                    let mut report = import_summary("folder-import-confirmed", confirmed.summary());
                    let Json::Object(fields) = &mut report else {
                        unreachable!("import summary is always an object")
                    };
                    fields.push((
                        "destination".to_owned(),
                        Json::text(confirmed.destination().to_string_lossy()),
                    ));
                    fields.push((
                        "receipt".to_owned(),
                        Json::text(confirmed.receipt().to_string_lossy()),
                    ));
                    fields.push(("private_history".to_owned(), Json::Bool(true)));
                    fields.push((
                        "operation".to_owned(),
                        Json::text(managed.operation().to_string()),
                    ));
                    fields.push((
                        "manifests".to_owned(),
                        Json::Number(managed.manifests() as u64),
                    ));
                    fields.push((
                        "materialized_entries".to_owned(),
                        Json::Number(managed.entries() as u64),
                    ));
                    fields.push((
                        "linked_bytes".to_owned(),
                        Json::Number(managed.linked_bytes()),
                    ));
                    fields.push(("shared_version_advanced".to_owned(), Json::Bool(false)));
                    println!("{report}");
                }
                Self::ImportRollback { destination } => {
                    let confirmed =
                        match mesh_daemon::ConfirmedFolderImport::open(Path::new(&destination)) {
                            Ok(confirmed) => confirmed,
                            Err(failure) => return Ok(refused(failure)),
                        };
                    let destination = confirmed.destination().to_path_buf();
                    if let Err(failure) = confirmed.rollback() {
                        return Ok(refused(failure));
                    }
                    println!(
                        "{}",
                        Json::object([
                            ("action", Json::text("folder-import-rolled-back")),
                            ("destination", Json::text(destination.to_string_lossy()),),
                        ])
                    );
                }
                Self::RestorePreview {
                    workspace,
                    object,
                    target,
                } => {
                    let root = Path::new(&workspace);
                    let record_file = root.join(mesh_daemon::RECORD_FILE_NAME);
                    if !record_file.is_file() {
                        return Err(format!(
                            "{workspace} has no {}, so there is no durable Mesh history to preview",
                            mesh_daemon::RECORD_FILE_NAME
                        ));
                    }
                    let open = mesh_daemon::OpenWorkspace::open(root)
                        .map_err(|error| format!("could not recover {workspace}: {error}"))?;
                    match open.preview_file_restore(object, target) {
                        Ok(preview) => println!("{}", preview.to_json()),
                        Err(refusal) => {
                            println!("{}", refusal.to_json());
                            return Ok(ExitCode::FAILURE);
                        }
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
    }

    /// The parsed command line.
    #[derive(Debug, Default)]
    struct Request {
        endpoint: Option<String>,
        plan: Option<Plan>,
        local: Option<Local>,
    }

    impl Request {
        fn parse(arguments: &[String]) -> Result<Self, String> {
            let mut request = Self::default();
            let mut rest = arguments.iter();
            while let Some(argument) = rest.next() {
                match argument.as_str() {
                    "-h" | "--help" => return Ok(Self::default()),
                    "--endpoint" => {
                        request.endpoint = Some(
                            rest.next()
                                .ok_or("`--endpoint` needs a path after it")?
                                .clone(),
                        );
                    }
                    "status" => request.plan = Some(simple("daemon.status")),
                    "describe" => request.plan = Some(simple("surface.describe")),
                    "startup" => request.plan = Some(simple("startup.report")),
                    "state" => request.plan = Some(simple("workspace.state")),
                    "open" => {
                        let folder = rest.next().ok_or("`open` needs a folder after it")?;
                        request.plan = Some(Plan {
                            method: "workspace.open",
                            params: Json::object([("path", Json::text(folder.clone()))]),
                            follow: 0,
                            output: Output::Wire,
                        });
                    }
                    "review-open" => {
                        let bundle = review_bundle(next(&mut rest, "review-open", "bundle")?)?;
                        let target = head(next(&mut rest, "review-open", "target")?)?;
                        let signer = load_signer(Path::new(next(
                            &mut rest,
                            "review-open",
                            "0600 key file",
                        )?))?;
                        let public = PublicKey::from_bytes(signer.verifying_key().to_bytes());
                        request.plan = Some(Plan {
                            method: "review.open",
                            params: Json::object([
                                ("bundle", Json::text(bundle.to_string())),
                                ("target", Json::text(target.to_string())),
                                (
                                    "opened_by",
                                    Json::text(public.actor_id::<Blake3>().to_string()),
                                ),
                            ]),
                            follow: 0,
                            output: Output::Wire,
                        });
                    }
                    "review-current" => {
                        let signer = load_signer(Path::new(next(
                            &mut rest,
                            "review-current",
                            "0600 key file",
                        )?))?;
                        let public = PublicKey::from_bytes(signer.verifying_key().to_bytes());
                        request.plan = Some(Plan {
                            method: "review.open-current",
                            params: Json::object([(
                                "opened_by",
                                Json::text(public.actor_id::<Blake3>().to_string()),
                            )]),
                            follow: 0,
                            output: Output::Wire,
                        });
                    }
                    "approve" => {
                        let _bundle = review_bundle(next(&mut rest, "approve", "bundle")?)?;
                        let _target = head(next(&mut rest, "approve", "target")?)?;
                        let parent_text = next(&mut rest, "approve", "current shared parent")?;
                        let _parent = if parent_text == "genesis" {
                            HeadId::from_bytes([0; 32])
                        } else {
                            head(parent_text)?
                        };
                        let _key_file = next(&mut rest, "approve", "human-held key")?;
                        return Err(
                            "`approve` is unavailable: this build has no verified human-held signing authority; no key was opened and nothing was sent"
                                .to_owned(),
                        );
                    }
                    "exclusions" => {
                        let workspace = rest
                            .next()
                            .ok_or("`exclusions` needs a workspace folder after it")?
                            .clone();
                        request.local = Some(Local::Exclusions {
                            workspace,
                            path: rest.next().cloned(),
                        });
                    }
                    "restrictions" => request.local = Some(Local::Restrictions),
                    "support-bundle" => {
                        let workspace = rest
                            .next()
                            .ok_or("`support-bundle` needs a workspace folder after it")?
                            .clone();
                        request.local = Some(Local::SupportBundle { workspace });
                    }
                    "import-preview" => {
                        request.local = Some(Local::ImportPreview {
                            source: next(&mut rest, "import-preview", "source folder")?.to_owned(),
                        });
                    }
                    "attach" => {
                        request.local = Some(Local::Attach {
                            source: next(&mut rest, "attach", "existing project folder")?
                                .to_owned(),
                            metadata: next(&mut rest, "attach", "external metadata folder")?
                                .to_owned(),
                        });
                    }
                    "attachment-status" | "attachment-observe" => {
                        request.local = Some(Local::AttachmentStatus {
                            observe: argument == "attachment-observe",
                            metadata: next(
                                &mut rest,
                                "attachment-status",
                                "external metadata folder",
                            )?
                            .to_owned(),
                        });
                    }
                    "import-confirm" => {
                        let source = next(&mut rest, "import-confirm", "source folder")?.to_owned();
                        let destination =
                            next(&mut rest, "import-confirm", "managed folder")?.to_owned();
                        let expected = Digest32::parse_hex(next(
                            &mut rest,
                            "import-confirm",
                            "preview summary",
                        )?)
                        .map_err(|error| format!("preview summary is not a digest: {error}"))?;
                        request.local = Some(Local::ImportConfirm {
                            source,
                            destination,
                            expected,
                        });
                    }
                    "import-rollback" => {
                        request.local = Some(Local::ImportRollback {
                            destination: next(&mut rest, "import-rollback", "managed folder")?
                                .to_owned(),
                        });
                    }
                    "restore-preview" => {
                        let workspace = next(&mut rest, "restore-preview", "workspace folder")?;
                        let object = ObjectId::parse(next(
                            &mut rest,
                            "restore-preview",
                            "object identifier",
                        )?)
                        .map_err(|error| format!("object identifier is invalid: {error}"))?;
                        let target = VersionId::parse(next(
                            &mut rest,
                            "restore-preview",
                            "target version identifier",
                        )?)
                        .map_err(|error| {
                            format!("target version identifier is invalid: {error}")
                        })?;
                        request.local = Some(Local::RestorePreview {
                            workspace: workspace.to_owned(),
                            object,
                            target,
                        });
                    }
                    "watch" => {
                        let how_many = rest.next().map_or(Ok(1), |value| {
                            value
                                .parse::<usize>()
                                .map_err(|_| format!("`watch` wants a count, not `{value}`"))
                        })?;
                        request.plan = Some(Plan {
                            method: "events.subscribe",
                            params: Json::empty_object(),
                            follow: how_many,
                            output: Output::Wire,
                        });
                    }
                    other => {
                        if let Some(value) = other.strip_prefix("--endpoint=") {
                            request.endpoint = Some(value.to_owned());
                        } else {
                            return Err(format!("`{other}` is not a command this client has"));
                        }
                    }
                }
            }
            request.route_endpoint_imports();
            Ok(request)
        }

        /// An endpoint makes import stateful and daemon-authoritative.
        ///
        /// The offline scanner cannot know that a selected folder is the daemon's open
        /// zero-history workspace, where old private database/CAS names must stay out of the new
        /// presented folder. Silently accepting `--endpoint` while ignoring it therefore produces
        /// a different preview and confirmation token from the desktop. Route the complete import
        /// transaction through the same live methods instead.
        fn route_endpoint_imports(&mut self) {
            if self.endpoint.is_none() {
                return;
            }
            let Some(local) = self.local.take() else {
                return;
            };
            self.local = match local {
                Local::ImportPreview { source } => {
                    self.plan = Some(Plan {
                        method: "folder.import.preview",
                        params: Json::object([("source", Json::text(source))]),
                        follow: 0,
                        output: Output::Wire,
                    });
                    None
                }
                Local::ImportConfirm {
                    source,
                    destination,
                    expected,
                } => {
                    self.plan = Some(Plan {
                        method: "folder.import.confirm",
                        params: Json::object([
                            ("source", Json::text(source)),
                            ("destination", Json::text(destination)),
                            ("summary", Json::text(expected.to_string())),
                        ]),
                        follow: 0,
                        output: Output::Wire,
                    });
                    None
                }
                Local::ImportRollback { destination } => {
                    self.plan = Some(Plan {
                        method: "folder.import.rollback",
                        params: Json::object([("destination", Json::text(destination))]),
                        follow: 0,
                        output: Output::Wire,
                    });
                    None
                }
                other => Some(other),
            };
        }
    }

    fn next<'a>(
        rest: &mut impl Iterator<Item = &'a String>,
        command: &str,
        field: &str,
    ) -> Result<&'a str, String> {
        rest.next()
            .map(String::as_str)
            .ok_or_else(|| format!("`{command}` needs a {field} after it"))
    }

    fn review_bundle(text: &str) -> Result<ReviewBundleId, String> {
        ReviewBundleId::parse_hex(text)
            .map_err(|error| format!("review bundle is not a 32-byte identifier: {error}"))
    }

    fn head(text: &str) -> Result<HeadId, String> {
        ApprovalDigest32::parse_hex(text)
            .map(|digest| HeadId::from_bytes(*digest.as_bytes()))
            .map_err(|error| format!("saved change is not a 32-byte identifier: {error}"))
    }

    fn load_signer(path: &Path) -> Result<SigningKey, String> {
        let mut file = File::open(path)
            .map_err(|error| format!("could not open the reviewer key file: {error}"))?;
        let metadata = file
            .metadata()
            .map_err(|error| format!("could not inspect the reviewer key file: {error}"))?;
        if !metadata.file_type().is_file() {
            return Err("the reviewer key path is not a regular file".to_owned());
        }
        if metadata.mode() & 0o777 != 0o600 {
            return Err("the reviewer key file must have permissions 0600".to_owned());
        }
        let mut seed = [0u8; 32];
        if file.read_exact(&mut seed).is_err() {
            seed.fill(0);
            return Err("the reviewer key file must contain exactly 32 bytes".to_owned());
        }
        let mut extra = [0u8; 1];
        match file.read(&mut extra) {
            Ok(0) => {}
            Ok(_) => {
                seed.fill(0);
                return Err("the reviewer key file must contain exactly 32 bytes".to_owned());
            }
            Err(error) => {
                seed.fill(0);
                return Err(format!(
                    "could not finish reading the reviewer key file: {error}"
                ));
            }
        }
        let signer = SigningKey::from_bytes(&seed);
        seed.fill(0);
        Ok(signer)
    }

    fn import_summary(action: &str, summary: &mesh_daemon::ImportSummary) -> Json {
        summary.to_json(action)
    }

    fn refused(problem: impl std::fmt::Display) -> ExitCode {
        eprintln!("meshctl: {problem}");
        ExitCode::FAILURE
    }

    fn same_workspace(expected: &Path, observed: &Path) -> Result<bool, String> {
        let expected = fs::metadata(expected)
            .map_err(|error| format!("could not identify the requested workspace: {error}"))?;
        let observed = fs::metadata(observed)
            .map_err(|error| format!("could not identify the service workspace: {error}"))?;
        Ok(expected.is_dir()
            && observed.is_dir()
            && expected.dev() == observed.dev()
            && expected.ino() == observed.ino())
    }

    /// A call with no parameters and nothing to wait for afterwards.
    const fn simple(method: &'static str) -> Plan {
        Plan {
            method,
            params: Json::empty_object(),
            follow: 0,
            output: Output::Wire,
        }
    }

    /// The help text, which is also the specification of the command line.
    const USAGE: &str = "\
meshctl — talk to a running Mesh background service

Usage: meshctl --endpoint <path> <command>

  status                 Is the service answering, and which interface does it speak?
  describe               Everything the service offers.
  startup                What it found when it last started.
  open <folder>          Open that folder as a workspace.
  state                  What the open workspace holds.
  support-bundle <folder>
                         Print the exact redacted support document from the matching live
                         workspace state already verified by this service.
  import-preview <source-folder>
                         Use the daemon's authenticated current-workspace scope when previewing.
  import-confirm <source-folder> <managed-folder> <preview-summary>
                         Confirm that exact daemon-scoped preview and open the managed copy.
  import-rollback <managed-folder>
                         Roll back through the daemon that may currently hold the managed copy.
  review-open <bundle> <target> <0600-key-file>
  review-current <0600-key-file>
                         Open or reuse an exact review with this human identity.
  approve <bundle> <target> <current-parent|genesis> <human-held-key>
      Unavailable in this build; no verified human-held signer is implemented.
  watch [count]          Subscribe, then print that many things as they happen (default 1).
  -h, --help             Show this message.

Answered without the service when --endpoint is omitted:

  attach <existing-project> <external-metadata-folder>
                         Register an existing project in place. Both folders must already exist
                         and use absolute paths. Does not start observation or capture versions.
  attachment-status <external-metadata-folder>
                         Reopen the attachment and refuse a replaced or missing project folder.
  attachment-observe <external-metadata-folder>
                         Read one bounded live inventory. Does not save a version or start watching.
  exclusions <folder> [path]
                         What that workspace does not version, and — when a path is
                         given — which source said so.
  restrictions           How Mesh will present a folder on this device, and everything
                         that way of working cannot see.
  support-bundle <folder>
                         Inspect a stopped or damaged folder without changing it. A live
                         workspace should use the endpoint form above.
  import-preview <source-folder>
                         Read and hash an existing folder without changing it or creating a copy.
  import-confirm <source-folder> <managed-folder> <preview-summary>
                         Copy, re-verify and confirm exactly the summary printed by preview.
  import-rollback <managed-folder>
                         Remove a confirmed managed copy only when its durable receipt, directory
                         identity and every copied byte still match.
  restore-preview <folder> <object-id> <target-version-id>
                         Recover durable local state and print the exact append-only operations
                         a restore would require. Never executes or authorizes the restore.

Every reply is printed as one line of JSON. Exit code: 0 answered, 1 refused, 2 could
not be reached.
";

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_command_with_no_endpoint_is_refused_before_anything_is_sent() {
            let problem = run(vec!["status".to_owned()]).expect_err("refused");
            assert!(problem.contains("--endpoint"), "{problem}");
        }

        #[test]
        fn open_carries_the_folder_as_a_parameter() {
            let request =
                Request::parse(&["open".to_owned(), "/tmp/w".to_owned()]).expect("parses");
            let plan = request.plan.expect("a plan");
            assert_eq!(plan.method, "workspace.open");
            assert_eq!(
                plan.params.get("path").and_then(Json::as_text),
                Some("/tmp/w")
            );
        }

        #[test]
        fn watch_defaults_to_one_and_reads_a_count_when_given() {
            let one = Request::parse(&["watch".to_owned()]).expect("parses");
            assert_eq!(one.plan.expect("a plan").follow, 1);
            let three = Request::parse(&["watch".to_owned(), "3".to_owned()]).expect("parses");
            assert_eq!(three.plan.expect("a plan").follow, 3);
        }

        #[test]
        fn a_count_that_is_not_a_number_is_refused_rather_than_defaulted() {
            assert!(Request::parse(&["watch".to_owned(), "soon".to_owned()]).is_err());
        }

        #[test]
        fn exclusions_is_answered_without_an_endpoint() {
            let request =
                Request::parse(&["exclusions".to_owned(), "/tmp/w".to_owned()]).expect("parses");
            assert!(request.plan.is_none(), "it does not call the service");
            assert!(matches!(
                request.local,
                Some(Local::Exclusions { ref path, .. }) if path.is_none()
            ));
            assert!(USAGE.contains("exclusions"), "`exclusions` is undocumented");
        }

        #[test]
        fn exclusions_carries_the_path_when_one_is_given() {
            let request = Request::parse(&[
                "exclusions".to_owned(),
                "/tmp/w".to_owned(),
                "target/debug/x".to_owned(),
            ])
            .expect("parses");
            let Some(Local::Exclusions { workspace, path }) = request.local else {
                panic!("no local command");
            };
            assert_eq!(workspace, "/tmp/w");
            assert_eq!(path.as_deref(), Some("target/debug/x"));
        }

        #[test]
        fn exclusions_without_a_folder_is_refused_before_anything_is_read() {
            let problem = Request::parse(&["exclusions".to_owned()]).expect_err("refused");
            assert!(problem.contains("workspace folder"), "{problem}");
        }

        #[test]
        fn restrictions_is_answered_without_an_endpoint_and_names_every_one() {
            let request = Request::parse(&["restrictions".to_owned()]).expect("parses");
            assert!(request.plan.is_none(), "it does not call the service");
            assert!(matches!(request.local, Some(Local::Restrictions)));
            assert!(
                USAGE.contains("restrictions"),
                "`restrictions` is undocumented"
            );
            // The command is only worth having if it carries the whole list, and the whole list is
            // the enumeration rather than seven strings written out here.
            let printed = mesh_daemon::choose_backend(mesh_daemon::Availability::probe())
                .to_json()
                .encode();
            for restriction in mesh_daemon::FallbackRestriction::ALL {
                assert!(printed.contains(restriction.id()), "{}", restriction.id());
                assert!(
                    printed.contains(restriction.headline()),
                    "{} has no sentence on this surface",
                    restriction.id()
                );
            }
        }

        #[test]
        fn support_bundle_is_a_local_exact_preview() {
            let request = Request::parse(&[
                "support-bundle".to_owned(),
                "/tmp/unhealthy-workspace".to_owned(),
            ])
            .expect("parses");
            assert!(request.plan.is_none(), "it does not call the service");
            assert!(matches!(
                request.local,
                Some(Local::SupportBundle { ref workspace })
                    if workspace == "/tmp/unhealthy-workspace"
            ));
            assert!(USAGE.contains("support-bundle"), "command is undocumented");
        }

        #[test]
        fn support_bundle_without_a_folder_is_refused_before_anything_is_read() {
            let problem = Request::parse(&["support-bundle".to_owned()]).expect_err("refused");
            assert!(problem.contains("workspace folder"), "{problem}");
        }

        #[test]
        fn endpoint_import_commands_use_the_daemon_instead_of_the_offline_scanner() {
            let preview = Request::parse(&[
                "--endpoint".to_owned(),
                "/private/tmp/mesh.sock".to_owned(),
                "import-preview".to_owned(),
                "/workspace".to_owned(),
            ])
            .expect("preview parses");
            assert!(preview.local.is_none(), "endpoint preview stayed offline");
            let preview = preview.plan.expect("daemon preview plan");
            assert_eq!(preview.method, "folder.import.preview");
            assert_eq!(
                preview.params.get("source").and_then(Json::as_text),
                Some("/workspace")
            );

            let summary = "11".repeat(32);
            let confirm = Request::parse(&[
                "import-confirm".to_owned(),
                "/workspace".to_owned(),
                "/managed".to_owned(),
                summary.clone(),
                "--endpoint=/private/tmp/mesh.sock".to_owned(),
            ])
            .expect("confirmation parses");
            assert!(
                confirm.local.is_none(),
                "endpoint confirmation stayed offline"
            );
            let confirm = confirm.plan.expect("daemon confirmation plan");
            assert_eq!(confirm.method, "folder.import.confirm");
            assert_eq!(
                confirm.params.get("source").and_then(Json::as_text),
                Some("/workspace")
            );
            assert_eq!(
                confirm.params.get("destination").and_then(Json::as_text),
                Some("/managed")
            );
            assert_eq!(
                confirm.params.get("summary").and_then(Json::as_text),
                Some(summary.as_str())
            );

            let rollback = Request::parse(&[
                "--endpoint".to_owned(),
                "/private/tmp/mesh.sock".to_owned(),
                "import-rollback".to_owned(),
                "/managed/mounts".to_owned(),
            ])
            .expect("rollback parses");
            assert!(rollback.local.is_none(), "endpoint rollback stayed offline");
            let rollback = rollback.plan.expect("daemon rollback plan");
            assert_eq!(rollback.method, "folder.import.rollback");
            assert_eq!(
                rollback.params.get("destination").and_then(Json::as_text),
                Some("/managed/mounts")
            );
        }

        #[test]
        fn restore_preview_is_local_requires_exact_ids_and_is_documented_as_non_executing() {
            let object = "11".repeat(16);
            let target = "22".repeat(32);
            let request = Request::parse(&[
                "restore-preview".to_owned(),
                "/tmp/workspace".to_owned(),
                object.clone(),
                target.clone(),
            ])
            .expect("parses");
            assert!(
                request.plan.is_none(),
                "it does not add an IPC write method"
            );
            assert!(matches!(
                request.local,
                Some(Local::RestorePreview { workspace, object: parsed_object, target: parsed_target })
                    if workspace == "/tmp/workspace"
                        && parsed_object.to_string() == object
                        && parsed_target.to_string() == target
            ));
            assert!(USAGE.contains("Never executes or authorizes the restore"));
        }

        #[test]
        fn restore_preview_refuses_malformed_or_missing_identity_before_opening_a_workspace() {
            let valid_object = "11".repeat(16);
            let missing = Request::parse(&[
                "restore-preview".to_owned(),
                "/tmp/workspace".to_owned(),
                valid_object.clone(),
            ])
            .expect_err("target is required");
            assert!(missing.contains("target version identifier"), "{missing}");

            let malformed = Request::parse(&[
                "restore-preview".to_owned(),
                "/tmp/workspace".to_owned(),
                valid_object,
                "not-a-version".to_owned(),
            ])
            .expect_err("malformed target is refused");
            assert!(malformed.contains("target version identifier is invalid"));
        }

        #[test]
        fn an_unknown_command_is_refused_and_named() {
            let problem = Request::parse(&["mount".to_owned()]).expect_err("refused");
            assert!(problem.contains("mount"), "{problem}");
        }

        #[test]
        fn approve_refuses_before_opening_a_software_key_or_connecting() {
            let missing_key = "/a/key/that/must/not/be/opened";
            let problem = Request::parse(&[
                "approve".to_owned(),
                "11".repeat(32),
                "22".repeat(32),
                "genesis".to_owned(),
                missing_key.to_owned(),
            ])
            .expect_err("software approval is unavailable");
            assert!(problem.contains("no verified human-held signing authority"));
            assert!(problem.contains("no key was opened"));
        }

        #[test]
        fn every_command_the_usage_names_is_a_method_the_catalogue_has() {
            for (command, method) in [
                ("status", "daemon.status"),
                ("describe", "surface.describe"),
                ("startup", "startup.report"),
                ("state", "workspace.state"),
            ] {
                assert!(USAGE.contains(command), "`{command}` is undocumented");
                assert!(
                    mesh_daemon::ipc::method(method).is_some(),
                    "`{command}` calls `{method}`, which is not on the catalogue"
                );
            }
            assert!(mesh_daemon::ipc::method("workspace.open").is_some());
            assert!(mesh_daemon::ipc::method("events.subscribe").is_some());
        }
    }
}
