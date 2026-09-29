//! Closed desktop remote-project requests; native history and signing retain all authority.
use super::*;
const INVALID: &str = "Invalid remote project selection";
fn hex(value: &str, n: usize) -> bool {
    value.len() == n
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
struct Request {
    value: Json,
    project: String,
    objective: String,
    offer: String,
    correlation: String,
    request: String,
    main: Option<String>,
    action: String,
}
impl Request {
    fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > 4096 {
            return Err(INVALID.into());
        }
        let value = Json::parse(raw).map_err(|_| INVALID)?;
        let Json::Object(fields) = &value else {
            return Err(INVALID.into());
        };
        let names = [
            "schema",
            "project",
            "objective",
            "offer",
            "correlation",
            "request",
            "expected_main",
            "action",
        ];
        if fields.len() != names.len()
            || names
                .iter()
                .any(|name| fields.iter().filter(|(key, _)| key == name).count() != 1)
        {
            return Err(INVALID.into());
        }
        let text = |name| {
            value
                .get(name)
                .and_then(Json::as_text)
                .ok_or(INVALID)
                .map(str::to_owned)
        };
        if text("schema")? != "mesh.desktop-remote-project-request/v1" {
            return Err(INVALID.into());
        }
        let project = text("project")?;
        let objective = text("objective")?;
        for id in [&project, &objective] {
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err(INVALID.into());
            }
        }
        let offer = text("offer")?;
        let correlation = text("correlation")?;
        let request = text("request")?;
        if !hex(&offer, 64) || !hex(&correlation, 64) || !hex(&request, 32) {
            return Err(INVALID.into());
        }
        let main = match value.get("expected_main") {
            Some(Json::Null) => None,
            Some(Json::Text(s)) if hex(s, 64) => Some(s.clone()),
            _ => return Err(INVALID.into()),
        };
        let action = text("action")?;
        if !matches!(
            action.as_str(),
            "stage" | "inspect_import" | "import" | "inspect_review" | "create_review"
        ) {
            return Err(INVALID.into());
        }
        Ok(Self {
            value,
            project,
            objective,
            offer,
            correlation,
            request,
            main,
            action,
        })
    }
}
impl AttachmentHost {
    /// Act only on an exact received result and independently selected native original project.
    pub fn remote_fleet_project(
        &self,
        raw: &str,
        trust: &mesh_daemon::TrustedReviewers,
    ) -> Result<String, String> {
        let selected = Request::parse(raw)?;
        #[cfg(target_os = "macos")]
        {
            let (offer, correlation) =
                Self::remote_review_ids(&selected.offer, &selected.correlation)?;
            let source = self.review_history(&selected.project)?;
            let history = self.fleet_history(&selected.objective)?;
            let request = mesh_daemon::fleet::RetainedRemoteProjectRequest {
                offer,
                correlation,
                source: &source,
                reviewers: trust,
                request: &selected.request,
                expected_main: selected.main.as_deref(),
            };
            let result = match selected.action.as_str() {
                "stage" => history
                    .stage_retained_remote_project(&request)
                    .map_err(|_| {
                        "Remote candidate could not be prepared; retain its exact selection"
                    })?,
                "inspect_import" | "import" => {
                    let recorded=history.recorded_retained_remote_project_import(&request).map_err(|_|"Remote import outcome could not be verified; retain its exact selection")?;
                    if selected.action == "inspect_import" {
                        recorded.map_or(Json::Null, |(_, outcome)| outcome)
                    } else if let Some((actor, outcome)) = recorded {
                        if outcome.get("state") == Some(&Json::text("imported")) {
                            outcome
                        } else {
                            let signer = NativeImportSigner::recorded(actor);
                            history.import_retained_remote_project(&request,&signer).map_err(|_|"Pending remote import could not resume; retain its exact selection")?
                        }
                    } else {
                        let signer = NativeImportSigner::fresh()?;
                        history
                            .import_retained_remote_project(&request, &signer)
                            .map_err(|_| {
                                "Remote import could not be confirmed; retain its exact selection"
                            })?
                    }
                }
                "inspect_review" | "create_review" => history
                    .review_retained_remote_project_import(
                        &request,
                        selected.action == "create_review",
                    )
                    .map_err(|_| {
                        "Imported remote review could not be verified; retain its exact selection"
                    })?,
                _ => return Err(INVALID.into()),
            };
            Ok(Json::object([
                (
                    "schema",
                    Json::text("mesh.desktop-remote-project-result/v1"),
                ),
                ("selection", selected.value),
                ("result", result),
            ])
            .encode())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (selected, trust);
            Err("Native remote project actions are unavailable on this platform".into())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input(action: &str) -> Json {
        Json::object([
            (
                "schema",
                Json::text("mesh.desktop-remote-project-request/v1"),
            ),
            ("project", Json::text("project")),
            ("objective", Json::text(format!("fleet-{}", "a".repeat(64)))),
            ("offer", Json::text("b".repeat(64))),
            ("correlation", Json::text("c".repeat(64))),
            ("request", Json::text("d".repeat(32))),
            ("expected_main", Json::Null),
            ("action", Json::text(action)),
        ])
    }
    #[test]
    fn remote_project_requests_are_closed_and_never_grant_approval_or_paths() {
        for action in [
            "stage",
            "inspect_import",
            "import",
            "inspect_review",
            "create_review",
        ] {
            assert!(Request::parse(&input(action).encode()).is_ok());
        }
        for action in ["approve", "apply", "launch", "retry", ""] {
            assert!(Request::parse(&input(action).encode()).is_err());
        }
        let Json::Object(fields) = input("stage") else {
            panic!()
        };
        for (key, value) in [
            ("project", Json::text("/tmp/project")),
            ("offer", Json::text("B".repeat(64))),
            ("request", Json::text("x".repeat(32))),
            ("expected_main", Json::Bool(false)),
        ] {
            let mut changed = fields.clone();
            changed.iter_mut().find(|(name, _)| name == key).unwrap().1 = value;
            assert!(Request::parse(&Json::Object(changed).encode()).is_err());
        }
        let mut extra = fields.clone();
        extra.push(("path".into(), Json::text("/tmp/work")));
        assert!(Request::parse(&Json::Object(extra).encode()).is_err());
        let mut duplicate = fields.clone();
        duplicate.push(fields[0].clone());
        assert!(Request::parse(&Json::Object(duplicate).encode()).is_err());
    }
    #[test]
    fn remote_project_actions_do_not_provision_missing_state() {
        let root = std::env::temp_dir().join(format!(
            "mesh-remote-project-command-{}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let host = AttachmentHost::new(&root);
        for action in [
            "stage",
            "inspect_import",
            "import",
            "inspect_review",
            "create_review",
        ] {
            assert!(host
                .remote_fleet_project(
                    &input(action).encode(),
                    &mesh_daemon::TrustedReviewers::default()
                )
                .is_err());
        }
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        drop(host);
        std::fs::remove_dir(root).unwrap();
    }
}
