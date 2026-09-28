//! Complete native file-change confirmation. Renderer content never supplies these facts.
use mesh_daemon::ipc::Json;
use std::path::Path;

const MAX_PROMPT: usize = 48 * 1024;

pub fn transaction_id(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "Native recovery identity is unavailable".into())
}

pub fn confirmation(
    action: &str,
    project: &str,
    root: &Path,
    proposal: &Json,
    current: &[u8],
    proposed: &[u8],
) -> Result<String, String> {
    let text = |bytes: &[u8]| {
        if bytes.len() > MAX_PROMPT || bytes.contains(&0) {
            return Err(
                "This file cannot be shown completely in the native text confirmation".to_owned(),
            );
        }
        std::str::from_utf8(bytes)
            .map_err(|_| "Binary files are not supported by this text confirmation".to_owned())
            // Escape control characters and delimit the complete content, including trailing lines.
            .map(|value| format!("{value:?}"))
    };
    let field = |key| {
        proposal
            .get(key)
            .and_then(Json::as_text)
            .ok_or_else(|| "Native file-change facts are incomplete".to_owned())
    };
    let mode = |key| {
        proposal
            .get(key)
            .and_then(Json::as_u64)
            .ok_or_else(|| "Native file permissions are unavailable".to_owned())
    };
    let absent =
        proposal.get("schema") == Some(&Json::text("mesh.attachment-file-restoration-addition/v1"));
    let (before, effect) = if absent {
        if !current.is_empty()
            || [
                "source_file",
                "source_digest",
                "source_mode",
                "source_executable",
            ]
            .iter()
            .any(|key| proposal.get(key) != Some(&Json::Null))
        {
            return Err("Native absent-file facts are inconsistent".into());
        }
        ("DESTINATION IS ABSENT\nA new file will be created at this path.".to_owned(),
         "This creates only this working file using the retained snapshot's permissions and metadata. A file created there by another tool will never be replaced. The original retained file stays available, including later writes through an already-open editor.")
    } else {
        (format!("CURRENT FILE TO PRESERVE\nDigest: {}\nPermissions: {:o}\n{}", field("source_digest")?, mode("source_mode")?, text(current)?),
         "This changes only this working file. Its current file is retained in private recovery storage, including later writes through an already-open editor. Existing retained files stay available.")
    };
    let prompt = format!(
        "{action}\n\nProject: {project:?}\nFolder: {root:?}\nFile: {:?}\n\n{before}\n\nCONTENT TO INSTALL\nDigest: {}\nPermissions: {:o}\n{}\n\n{effect} Mesh main and Git are unchanged. Restored content remains private work.\n\nThe exact inputs will be checked again after confirmation. A concurrent change can require recovery inspection. Cancel leaves your working file unchanged and retains the prepared record.",
        field("path")?, field("installed_digest")?, mode("installed_mode")?, text(proposed)?,
    );
    if prompt.len() > MAX_PROMPT {
        return Err("This change is too large to show completely in the native confirmation. No content will be omitted.".into());
    }
    Ok(prompt)
}

pub fn group_confirmation(
    project: &str,
    root: &Path,
    prepared: &mesh_daemon::project_attachment::PreparedMainIntegration,
) -> Result<String, String> {
    let files: Vec<_> = prepared
        .files()
        .map(|file| {
            (
                file.proposal(),
                file.current_content(),
                file.proposed_content(),
            )
        })
        .collect();
    group_prompt(project, root, prepared.proposal(), &files)
}

fn group_prompt(
    project: &str,
    root: &Path,
    proposal: &Json,
    files: &[(&Json, &[u8], &[u8])],
) -> Result<String, String> {
    let fail = || "The complete group cannot be shown in native confirmation".to_owned();
    let members = proposal
        .get("members")
        .and_then(Json::as_array)
        .ok_or_else(fail)?;
    let present = proposal
        .get("already_present")
        .and_then(Json::as_array)
        .ok_or_else(fail)?;
    if files.is_empty() || files.len() != members.len() || files.len() + present.len() > 64 {
        return Err(fail());
    }
    let mut prompt = format!("Apply accepted changes to the working folder\n\nProject: {project:?}\nFolder: {root:?}\n\nEvery change below belongs to one accepted review. Changes are applied one at a time. Concurrent work can stop the group after some files have changed. Displaced files remain in recovery, including later writes from open editors. No automatic rollback or retry occurs. Mesh main and Git are unchanged.\n");
    let side = |receipt: &Json, prefix: &str, bytes: &[u8]| -> Result<String, String> {
        if bytes.len() > MAX_PROMPT || bytes.contains(&0) {
            return Err(fail());
        }
        let content = std::str::from_utf8(bytes).map_err(|_| fail())?;
        let digest = receipt
            .get(&format!("{prefix}_digest"))
            .and_then(Json::as_text)
            .ok_or_else(fail)?;
        let mode = receipt
            .get(&format!("{prefix}_mode"))
            .and_then(Json::as_u64)
            .ok_or_else(fail)?;
        Ok(format!(
            "Digest: {digest}\nPermissions: {mode:o}\n{content:?}"
        ))
    };
    for ((receipt, current, proposed), member) in files.iter().zip(members) {
        let path = receipt
            .get("path")
            .and_then(Json::as_text)
            .ok_or_else(fail)?;
        if member.get("path") != receipt.get("path") {
            return Err(fail());
        }
        let schema = receipt
            .get("schema")
            .and_then(Json::as_text)
            .ok_or_else(fail)?;
        let (action, before, after) = match schema {
            "mesh.attachment-file-addition/v2" => {
                if !current.is_empty() || receipt.get("source_file") != Some(&Json::Null) {
                    return Err(fail());
                }
                (
                    "CREATE",
                    "Destination absent; a concurrent file will not be replaced.".to_owned(),
                    side(receipt, "installed", proposed)?,
                )
            }
            "mesh.attachment-file-removal/v1" => {
                if !proposed.is_empty() || receipt.get("installed_file") != Some(&Json::Null) {
                    return Err(fail());
                }
                (
                    "REMOVE",
                    side(receipt, "source", current)?,
                    "File will be moved to retained recovery.".to_owned(),
                )
            }
            "mesh.attachment-file-integration/v1" => (
                "REPLACE",
                side(receipt, "source", current)?,
                side(receipt, "installed", proposed)?,
            ),
            _ => return Err(fail()),
        };
        prompt.push_str(&format!(
            "\n{action} {path:?}\nBEFORE\n{before}\nAFTER\n{after}\n"
        ));
        if prompt.len() > MAX_PROMPT {
            return Err(fail());
        }
    }
    for item in present {
        prompt.push_str(&format!(
            "\nALREADY PRESENT (no write): {:?}\n",
            item.as_text().ok_or_else(fail)?
        ));
    }
    prompt.push_str("\nExact inputs are checked again after confirmation and before each change. Cancel leaves the working folder unchanged and retains prepared recovery records.");
    if prompt.len() > MAX_PROMPT {
        return Err(fail());
    }
    Ok(prompt)
}

pub fn result(project: &str, transaction: &str, outcome: Json) -> String {
    Json::object([
        (
            "schema",
            Json::text("mesh.desktop-attachment-file-change/v1"),
        ),
        ("project", Json::text(project)),
        ("transaction", Json::text(transaction)),
        ("outcome", outcome),
    ])
    .encode()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal() -> Json {
        Json::object([
            ("path", Json::text("work\nFILE.txt")),
            ("source_digest", Json::text("before")),
            ("installed_digest", Json::text("after")),
            ("source_mode", Json::Number(0o644)),
            ("installed_mode", Json::Number(0o600)),
        ])
    }

    #[test]
    fn confirmation_keeps_project_and_folder_labels_literal() {
        let prompt = confirmation(
            "Restore",
            "project\nCONTENT TO INSTALL",
            Path::new("/workspace/שם\nPermissions: 777"),
            &proposal(),
            b"before",
            b"after",
        )
        .unwrap();
        assert!(prompt.contains(r#"Project: "project\nCONTENT TO INSTALL""#));
        assert!(prompt.contains(r#"Folder: "/workspace/שם\nPermissions: 777""#));
        assert!(!prompt.contains("\nPermissions: 777"));
    }

    #[test]
    fn confirmation_delimits_control_text_and_refuses_omitted_content() {
        let prompt = confirmation(
            "Restore",
            "project",
            Path::new("/fixture"),
            &proposal(),
            b"before\n",
            b"\"after\"\t",
        )
        .unwrap();
        assert!(prompt.contains("File: \"work\\nFILE.txt\""));
        assert!(prompt.contains("\"before\\n\""));
        assert!(prompt.contains("\"\\\"after\\\"\\t\""));
        assert!(prompt.contains("Permissions: 644"));
        assert!(confirmation(
            "Restore",
            "project",
            Path::new("/fixture"),
            &proposal(),
            &[255],
            b"after"
        )
        .is_err());
        assert!(confirmation(
            "Restore",
            "project",
            Path::new("/fixture"),
            &proposal(),
            b"\0",
            b"after"
        )
        .is_err());
        assert!(confirmation(
            "Restore",
            "project",
            Path::new("/fixture"),
            &proposal(),
            &vec![b'a'; MAX_PROMPT],
            b"after"
        )
        .is_err());
        assert!(confirmation(
            "Restore",
            "project",
            Path::new("/fixture"),
            &Json::Null,
            b"before",
            b"after"
        )
        .is_err());
    }
    #[test]
    fn absent_restoration_confirmation_describes_creation_and_rejects_inconsistent_facts() {
        let mut proposal = proposal();
        let Json::Object(fields) = &mut proposal else {
            unreachable!()
        };
        fields.push((
            "schema".into(),
            Json::text("mesh.attachment-file-restoration-addition/v1"),
        ));
        for key in [
            "source_file",
            "source_digest",
            "source_mode",
            "source_executable",
        ] {
            fields.retain(|(name, _)| name != key);
            fields.push((key.into(), Json::Null));
        }
        let prompt = confirmation(
            "Restore",
            "project",
            Path::new("/fixture"),
            &proposal,
            b"",
            b"private retained work",
        )
        .unwrap();
        assert!(prompt.contains("DESTINATION IS ABSENT"));
        assert!(prompt.contains("will never be replaced"));
        assert!(prompt.contains("private retained work"));
        assert!(!prompt.contains("CURRENT FILE TO PRESERVE"));
        assert!(confirmation(
            "Restore",
            "project",
            Path::new("/fixture"),
            &proposal,
            b"existing work",
            b"retained"
        )
        .is_err());
        let Json::Object(fields) = &mut proposal else {
            unreachable!()
        };
        fields
            .iter_mut()
            .find(|(name, _)| name == "source_digest")
            .unwrap()
            .1 = Json::text("unexpected");
        assert!(confirmation(
            "Restore",
            "project",
            Path::new("/fixture"),
            &proposal,
            b"",
            b"retained"
        )
        .is_err());
    }
    #[test]
    fn group_confirmation_shows_every_action_and_refuses_partial_or_unrenderable_content() {
        let receipt = |schema: &str, path: &str| {
            Json::object([
                ("schema", Json::text(schema)),
                ("path", Json::text(path)),
                (
                    "source_file",
                    if schema.contains("addition") {
                        Json::Null
                    } else {
                        Json::text("old")
                    },
                ),
                (
                    "installed_file",
                    if schema.contains("removal") {
                        Json::Null
                    } else {
                        Json::text("new")
                    },
                ),
                ("source_digest", Json::text("before")),
                ("installed_digest", Json::text("after")),
                ("source_mode", Json::Number(0o100644)),
                ("installed_mode", Json::Number(0o100755)),
            ])
        };
        let add = receipt("mesh.attachment-file-addition/v2", "new.txt");
        let replace = receipt("mesh.attachment-file-integration/v1", "edited.txt");
        let remove = receipt("mesh.attachment-file-removal/v1", "old\nfile.txt");
        let proposal = Json::object([
            (
                "members",
                Json::Array(
                    [&add, &replace, &remove]
                        .map(|file| Json::object([("path", file.get("path").unwrap().clone())]))
                        .to_vec(),
                ),
            ),
            (
                "already_present",
                Json::Array(vec![Json::text("present.txt")]),
            ),
        ]);
        let files: &[(&Json, &[u8], &[u8])] = &[
            (&add, b"", b"new"),
            (&replace, b"old edit", b"new edit"),
            (&remove, b"removed work", b""),
        ];
        let prompt = group_prompt("project", Path::new("/fixture"), &proposal, files).unwrap();
        let literal = group_prompt(
            "project\nCREATE fake",
            Path::new("/workspace/שם\nREMOVE fake"),
            &proposal,
            files,
        )
        .unwrap();
        assert!(literal.contains(r#"Project: "project\nCREATE fake""#));
        assert!(literal.contains(r#"Folder: "/workspace/שם\nREMOVE fake""#));
        assert!(!literal.contains("\nCREATE fake"));
        assert!(!literal.contains("\nREMOVE fake"));
        for expected in [
            "CREATE",
            "REPLACE",
            "REMOVE",
            "old\\nfile.txt",
            "present.txt",
            "removed work",
            "new edit",
            "one at a time",
            "No automatic rollback",
        ] {
            assert!(prompt.contains(expected), "{expected}");
        }
        assert!(group_prompt("project", Path::new("/fixture"), &proposal, &files[..2]).is_err());
        let binary: &[(&Json, &[u8], &[u8])] = &[(&add, b"", &[255]), files[1], files[2]];
        assert!(group_prompt("project", Path::new("/fixture"), &proposal, binary).is_err());
        let too_large = vec![b'x'; MAX_PROMPT];
        assert!(group_prompt(
            "project",
            Path::new("/fixture"),
            &proposal,
            &[(&add, b"", &too_large), files[1], files[2]]
        )
        .is_err());
        assert!(group_prompt(
            "project",
            Path::new("/fixture"),
            &proposal,
            &[files[1], files[0], files[2]]
        )
        .is_err());
    }
}
