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

/// Only native prepared content is eligible for confirmation; renderer facts never enter this path.
pub fn entry_restoration_confirmation(
    project: &str,
    root: &Path,
    prepared: &mesh_daemon::project_attachment::PreparedRetainedEntryRestoration,
) -> Result<String, String> {
    entry_restoration_prompt(
        project,
        root,
        prepared.proposal(),
        &prepared.restored_files().collect::<Vec<_>>(),
        &prepared.current_files().collect::<Vec<_>>(),
    )
}

fn entry_restoration_prompt(
    project: &str,
    root: &Path,
    proposal: &Json,
    restored: &[(&str, &[u8], bool)],
    current: &[(&str, &[u8], bool)],
) -> Result<String, String> {
    let fail = || "Complete restoration facts are unavailable for native confirmation".to_owned();
    if proposal.get("schema") != Some(&Json::text("mesh.attachment-entry-restoration/v1"))
        || proposal.get("project") != Some(&Json::text(project))
        || proposal.get("automatic_replay") != Some(&Json::Bool(false))
    {
        return Err(fail());
    }
    let field = |key| proposal.get(key).and_then(Json::as_text).ok_or_else(fail);
    let original = proposal
        .get("origin_tree")
        .and_then(Json::as_array)
        .ok_or_else(fail)?;
    let installed = proposal
        .get("installed_tree")
        .and_then(Json::as_array)
        .ok_or_else(fail)?;
    if original.len() != installed.len()
        || original.iter().zip(installed).any(|(a, b)| {
            ["path", "kind", "mode", "metadata", "digest", "bytes"]
                .iter()
                .any(|key| a.get(key).is_none() || a.get(key) != b.get(key))
        })
    {
        return Err(fail());
    }
    let before = match proposal.get("current_tree") {
        Some(Json::Null) if current.is_empty() => {
            "DESTINATION IS ABSENT\nA concurrent entry will never be replaced.".to_owned()
        }
        Some(Json::Array(entries)) => format!(
            "CURRENT ENTRY TO PRESERVE\n{}",
            entries_prompt(entries, current)?
        ),
        _ => return Err(fail()),
    };
    let prompt = format!(
        "Restore retained file or folder\n\nProject: {project:?}\nFolder: {root:?}\nEntry: {:?}\nOrigin recovery reference: {:?}\n\n{before}\n\nRETAINED SNAPSHOT TO COPY\n{}\nA fresh copy of this complete snapshot will be installed. The original retained objects and their open handles stay in their original recovery location. Any current destination is preserved in a new recovery transaction, including later writes through its open handles. No automatic undo, replay or cleanup occurs. Mesh main and Git are unchanged; restored content remains private work.\n\nThe exact inputs and project generation will be checked again after confirmation. Cancel leaves working files unchanged and retains the prepared record.",
        field("path")?, field("origin_transaction")?, entries_prompt(original, restored)?,
    );
    if prompt.len() > MAX_PROMPT {
        return Err(fail());
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
    let trees: Vec<_> = prepared
        .directories()
        .map(|tree| {
            (
                tree.proposal(),
                tree.confirmation_files().collect(),
                tree.current_files().collect(),
            )
        })
        .collect();
    group_prompt_with_trees(project, root, prepared.proposal(), &files, &trees)
}

type FrozenFiles<'a> = Vec<(&'a str, &'a [u8], bool)>;
type DirectoryContents<'a> = (&'a Json, FrozenFiles<'a>, FrozenFiles<'a>);

#[cfg(test)]
fn group_prompt(
    project: &str,
    root: &Path,
    proposal: &Json,
    files: &[(&Json, &[u8], &[u8])],
) -> Result<String, String> {
    group_prompt_with_trees(project, root, proposal, files, &[])
}

fn group_prompt_with_trees(
    project: &str,
    root: &Path,
    proposal: &Json,
    files: &[(&Json, &[u8], &[u8])],
    trees: &[DirectoryContents<'_>],
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
    if members.is_empty()
        || files.len() + trees.len() != members.len()
        || members.len() + present.len() > 64
    {
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
    let mut file_iter = files.iter();
    let mut tree_iter = trees.iter();
    let mut remaining_entries = 64usize.saturating_sub(present.len());
    for member in members {
        if trees
            .iter()
            .any(|(tree, ..)| tree.get("path") == member.get("path"))
        {
            let (tree, content, before) = tree_iter.next().ok_or_else(fail)?;
            if tree.get("path") != member.get("path") {
                return Err(fail());
            }
            let entries = tree.get("tree").and_then(Json::as_array).ok_or_else(fail)?;
            let converted =
                tree.get("schema") == Some(&Json::text("mesh.attachment-entry-conversion/v1"));
            let count = if converted {
                let original = tree
                    .get("before_tree")
                    .and_then(Json::as_array)
                    .ok_or_else(fail)?;
                original
                    .iter()
                    .chain(entries)
                    .map(|entry| entry.get("path").and_then(Json::as_text).ok_or_else(fail))
                    .collect::<Result<std::collections::BTreeSet<_>, _>>()?
                    .len()
            } else {
                entries.len()
            };
            remaining_entries = remaining_entries.checked_sub(count).ok_or_else(fail)?;
            prompt.push_str(&if converted {
                conversion_prompt(tree, before, content)?
            } else {
                directory_prompt(tree, content)?
            });
            if prompt.len() > MAX_PROMPT {
                return Err(fail());
            }
            continue;
        }
        remaining_entries = remaining_entries.checked_sub(1).ok_or_else(fail)?;
        let (receipt, current, proposed) = file_iter.next().ok_or_else(fail)?;
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
    if file_iter.next().is_some() || tree_iter.next().is_some() {
        return Err(fail());
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

fn directory_prompt(proposal: &Json, content: &[(&str, &[u8], bool)]) -> Result<String, String> {
    let fail =
        || "The complete directory tree cannot be shown in native text confirmation".to_owned();
    let removed =
        proposal.get("schema") == Some(&Json::text("mesh.attachment-directory-removal/v1"));
    if !removed
        && proposal.get("schema") != Some(&Json::text("mesh.attachment-directory-addition/v1"))
    {
        return Err(fail());
    }
    let root = proposal
        .get("path")
        .and_then(Json::as_text)
        .ok_or_else(fail)?;
    let entries = proposal
        .get("tree")
        .and_then(Json::as_array)
        .ok_or_else(fail)?;
    if entries.is_empty() || entries.len() > 64 {
        return Err(fail());
    }
    let mut prompt = if removed {
        format!("\nREMOVE DIRECTORY TREE {root:?}\nThe complete existing tree below will move to retained recovery. Files are never individually deleted. Open file and directory handles remain attached to that retained tree. Changed or extra entries require another review; no automatic cleanup or replay occurs.\n")
    } else {
        format!("\nCREATE DIRECTORY TREE {root:?}\nDestination absent; a concurrent entry will not be replaced. The complete tree is installed together.\n")
    };
    prompt.push_str(&entries_prompt(entries, content)?);
    if prompt.len() > MAX_PROMPT {
        return Err(fail());
    }
    Ok(prompt)
}

fn entries_prompt(entries: &[Json], content: &[(&str, &[u8], bool)]) -> Result<String, String> {
    use mesh_types::ContentDigest as _;
    let fail = || "Complete entry content is unavailable for native confirmation".to_owned();
    if entries.is_empty() || entries.len() > 64 {
        return Err(fail());
    }
    let mut files = content.iter();
    let mut prompt = String::new();
    for entry in entries {
        let path = entry.get("path").and_then(Json::as_text).ok_or_else(fail)?;
        let mode = entry.get("mode").and_then(Json::as_u64).ok_or_else(fail)?;
        match entry.get("kind").and_then(Json::as_text) {
            Some("directory") => {
                prompt.push_str(&format!("DIRECTORY {path:?} Permissions: {mode:o}\n"))
            }
            Some("file") => {
                let (file_path, bytes, executable) = files.next().ok_or_else(fail)?;
                if *file_path != path
                    || bytes.len() > MAX_PROMPT
                    || bytes.contains(&0)
                    || *executable != (mode & 0o111 != 0)
                    || entry.get("bytes") != Some(&Json::Number(bytes.len() as u64))
                    || entry.get("digest")
                        != Some(&Json::text(
                            mesh_types::Blake3::digest_bytes(bytes).to_string(),
                        ))
                {
                    return Err(fail());
                }
                let text = std::str::from_utf8(bytes).map_err(|_| fail())?;
                prompt.push_str(&format!(
                    "FILE {path:?}\nDigest: {}\nPermissions: {mode:o}\n{text:?}\n",
                    entry
                        .get("digest")
                        .and_then(Json::as_text)
                        .ok_or_else(fail)?
                ));
            }
            _ => return Err(fail()),
        }
        if prompt.len() > MAX_PROMPT {
            return Err(fail());
        }
    }
    if files.next().is_some() {
        return Err(fail());
    }
    Ok(prompt)
}

fn conversion_prompt(
    proposal: &Json,
    before: &[(&str, &[u8], bool)],
    after: &[(&str, &[u8], bool)],
) -> Result<String, String> {
    let fail = || "The complete conversion cannot be shown in native confirmation".to_owned();
    if proposal.get("schema") != Some(&Json::text("mesh.attachment-entry-conversion/v1")) {
        return Err(fail());
    }
    let root = proposal
        .get("path")
        .and_then(Json::as_text)
        .ok_or_else(fail)?;
    let original = proposal
        .get("before_tree")
        .and_then(Json::as_array)
        .ok_or_else(fail)?;
    let replacement = proposal
        .get("tree")
        .and_then(Json::as_array)
        .ok_or_else(fail)?;
    let direction = match (
        original
            .first()
            .and_then(|entry| entry.get("kind"))
            .and_then(Json::as_text),
        replacement
            .first()
            .and_then(|entry| entry.get("kind"))
            .and_then(Json::as_text),
    ) {
        (Some("file"), Some("directory")) => "FILE TO FOLDER",
        (Some("directory"), Some("file")) => "FOLDER TO FILE",
        _ => return Err(fail()),
    };
    let prompt = format!("\nCONVERT {direction} {root:?}\nCURRENT ENTRY TO RETAIN\n{}\nREPLACEMENT TO INSTALL\n{}\nOne native exchange retains the original entry and its open handles in recovery. No remove-then-create step, automatic rollback or replay occurs.\n", entries_prompt(original, before)?, entries_prompt(replacement, after)?);
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
    #[test]
    fn directory_confirmation_covers_empty_folders_and_every_frozen_file() {
        use mesh_types::ContentDigest as _;
        let directory = |path: &str| {
            Json::object([
                ("path", Json::text(path)),
                ("kind", Json::text("directory")),
                ("mode", Json::Number(0o040700)),
            ])
        };
        let file = Json::object([
            ("path", Json::text("run")),
            ("kind", Json::text("file")),
            ("mode", Json::Number(0o100700)),
            ("bytes", Json::Number(4)),
            (
                "digest",
                Json::text(mesh_types::Blake3::digest_bytes(b"text").to_string()),
            ),
        ]);
        let receipt = Json::object([
            (
                "schema",
                Json::text("mesh.attachment-directory-addition/v1"),
            ),
            ("path", Json::text("new")),
            (
                "tree",
                Json::Array(vec![directory(""), directory("empty"), file]),
            ),
        ]);
        let proposal = Json::object([
            (
                "members",
                Json::Array(vec![Json::object([("path", Json::text("new"))])]),
            ),
            ("already_present", Json::Array(vec![])),
        ]);
        let content: Vec<(&str, &[u8], bool)> = vec![("run", b"text", true)];
        let prompt = group_prompt_with_trees(
            "project",
            Path::new("/tmp/project"),
            &proposal,
            &[],
            &[(&receipt, content.clone(), vec![])],
        )
        .unwrap();
        for expected in [
            "CREATE DIRECTORY TREE",
            "DIRECTORY \"empty\"",
            "FILE \"run\"",
            "text",
            "100700",
            "concurrent entry will not be replaced",
        ] {
            assert!(prompt.contains(expected), "{expected}");
        }
        assert!(
            group_prompt_with_trees("project", Path::new("/tmp/project"), &proposal, &[], &[])
                .is_err()
        );
        let literal = group_prompt_with_trees(
            "project\nCREATE fake",
            Path::new("/workspace/שם\nPermissions: 777"),
            &proposal,
            &[],
            &[(&receipt, content.clone(), vec![])],
        )
        .unwrap();
        assert!(literal.contains(r#"Project: "project\nCREATE fake""#));
        assert!(literal.contains(r#"Folder: "/workspace/שם\nPermissions: 777""#));
        assert!(!literal.contains("\nCREATE fake\n"));
        let Json::Object(fields) = receipt.clone() else {
            panic!("receipt object")
        };
        let removal = Json::object(fields.into_iter().map(|(key, value)| {
            let value = if key == "schema" {
                Json::text("mesh.attachment-directory-removal/v1")
            } else {
                value
            };
            (key, value)
        }));
        let removal_prompt = directory_prompt(&removal, &content).unwrap();
        assert!(removal_prompt.contains("REMOVE DIRECTORY TREE"));
        assert!(removal_prompt.contains("move to retained recovery"));
        assert!(removal_prompt.contains("Open file and directory handles"));
        assert!(removal_prompt.contains("empty"));
        assert!(!removal_prompt.contains("CREATE DIRECTORY TREE"));
        assert!(directory_prompt(&receipt, &[]).is_err());
        assert!(directory_prompt(&receipt, &[("run", b"edit", true)]).is_err());
        assert!(directory_prompt(&receipt, &[("run", b"text", false)]).is_err());
        assert!(directory_prompt(
            &receipt,
            &[("run", b"text", true), ("extra", b"extra", false)]
        )
        .is_err());
        assert!(directory_prompt(&receipt, &[("run", &[255], true)]).is_err());
        assert!(directory_prompt(&receipt, &[("run", &vec![b'x'; MAX_PROMPT + 1], true)]).is_err());
    }
    #[test]
    fn conversion_confirmation_includes_complete_original_and_replacement() {
        use mesh_types::ContentDigest as _;
        let file = |path: &str, bytes: &[u8]| {
            Json::object([
                ("path", Json::text(path)),
                ("kind", Json::text("file")),
                ("mode", Json::Number(0o100600)),
                ("bytes", Json::Number(bytes.len() as u64)),
                (
                    "digest",
                    Json::text(mesh_types::Blake3::digest_bytes(bytes).to_string()),
                ),
            ])
        };
        let folder = |path: &str| {
            Json::object([
                ("path", Json::text(path)),
                ("kind", Json::text("directory")),
                ("mode", Json::Number(0o040700)),
            ])
        };
        for directory_before in [false, true] {
            let singleton = Json::Array(vec![file("", b"single")]);
            let tree = Json::Array(vec![folder(""), folder("empty"), file("file", b"nested")]);
            let receipt = Json::object([
                ("schema", Json::text("mesh.attachment-entry-conversion/v1")),
                ("path", Json::text("entry")),
                (
                    "before_tree",
                    if directory_before {
                        tree.clone()
                    } else {
                        singleton.clone()
                    },
                ),
                ("tree", if directory_before { singleton } else { tree }),
            ]);
            let single: Vec<(&str, &[u8], bool)> = vec![("", b"single", false)];
            let nested: Vec<(&str, &[u8], bool)> = vec![("file", b"nested", false)];
            let (before, after) = if directory_before {
                (&nested, &single)
            } else {
                (&single, &nested)
            };
            let proposal = Json::object([
                (
                    "members",
                    Json::Array(vec![Json::object([("path", Json::text("entry"))])]),
                ),
                ("already_present", Json::Array(vec![])),
            ]);
            let text = group_prompt_with_trees(
                "project",
                Path::new("/workspace/שם\nCONVERT fake"),
                &proposal,
                &[],
                &[(&receipt, after.clone(), before.clone())],
            )
            .unwrap();
            for expected in [
                "CONVERT",
                "CURRENT ENTRY TO RETAIN",
                "REPLACEMENT TO INSTALL",
                "single",
                "nested",
                "empty",
                "One native exchange",
            ] {
                assert!(text.contains(expected), "{expected}");
            }
            assert!(text.contains(r#"Folder: "/workspace/שם\nCONVERT fake""#));
            assert!(text.contains(if directory_before {
                "FOLDER TO FILE"
            } else {
                "FILE TO FOLDER"
            }));
            assert!(conversion_prompt(&receipt, &[], after).is_err());
            assert!(conversion_prompt(&receipt, before, &[]).is_err());
            assert!(conversion_prompt(&receipt, &[("", &[0], false)], after).is_err());
        }
    }
    fn restoration_tree(directory: bool, bytes: &[u8]) -> Json {
        use mesh_types::ContentDigest as _;
        let directory_entry = |path| {
            Json::object([
                ("path", Json::text(path)),
                ("kind", Json::text("directory")),
                ("mode", Json::Number(0o040700)),
                ("metadata", Json::text("metadata")),
                ("digest", Json::Null),
                ("bytes", Json::Null),
            ])
        };
        let file = Json::object([
            (
                "path",
                Json::text(if directory { "שם/file\nname" } else { "" }),
            ),
            ("kind", Json::text("file")),
            ("mode", Json::Number(0o100600)),
            ("metadata", Json::text("metadata")),
            (
                "digest",
                Json::text(mesh_types::Blake3::digest_bytes(bytes).to_string()),
            ),
            ("bytes", Json::Number(bytes.len() as u64)),
        ]);
        Json::Array(if directory {
            vec![
                directory_entry(""),
                directory_entry("empty"),
                directory_entry("שם"),
                file,
            ]
        } else {
            vec![file]
        })
    }
    fn restoration_receipt(original: Json, current: Json) -> Json {
        Json::object([
            ("schema", Json::text("mesh.attachment-entry-restoration/v1")),
            ("project", Json::text("project")),
            ("path", Json::text("שם/entry\nRESTORE fake")),
            ("origin_transaction", Json::text("directory-reference")),
            ("automatic_replay", Json::Bool(false)),
            ("origin_tree", original.clone()),
            ("installed_tree", original),
            ("current_tree", current),
        ])
    }
    #[test]
    fn entry_restoration_confirmation_shows_both_complete_sides_and_literal_identity() {
        for directory in [false, true] {
            for current_directory in [None, Some(false), Some(true)] {
                let original = restoration_tree(directory, b"retained\ntext");
                let current =
                    current_directory.map_or(Json::Null, |kind| restoration_tree(kind, b"current"));
                let proposal = restoration_receipt(original, current);
                let restored: Vec<(&str, &[u8], bool)> = vec![(
                    if directory { "שם/file\nname" } else { "" },
                    b"retained\ntext",
                    false,
                )];
                let current: Vec<(&str, &[u8], bool)> = current_directory.map_or(vec![], |kind| {
                    vec![(
                        if kind { "שם/file\nname" } else { "" },
                        &b"current"[..],
                        false,
                    )]
                });
                let prompt = entry_restoration_prompt(
                    "project",
                    Path::new("/שם\nFolder"),
                    &proposal,
                    &restored,
                    &current,
                )
                .unwrap();
                assert!(prompt.contains("retained\\ntext"));
                assert!(prompt.contains("שם/entry\\nRESTORE fake"));
                assert!(prompt.contains("/שם\\nFolder"));
                assert!(prompt.contains("Mesh main and Git are unchanged"));
                assert!(prompt.contains("original retained objects"));
                if directory || current_directory == Some(true) {
                    assert!(prompt.contains("DIRECTORY \"empty\""));
                }
                if current_directory.is_none() {
                    assert!(prompt.contains("DESTINATION IS ABSENT"));
                } else {
                    assert!(prompt.contains("CURRENT ENTRY TO PRESERVE"));
                    assert!(prompt.contains("\"current\""));
                }
            }
        }
    }
    #[test]
    fn entry_restoration_confirmation_refuses_missing_changed_or_unrenderable_sides() {
        let tree = restoration_tree(false, b"text");
        let proposal = restoration_receipt(tree.clone(), tree);
        let files: Vec<(&str, &[u8], bool)> = vec![("", b"text", false)];
        let prompt =
            |proposal: &Json, restored: &[(&str, &[u8], bool)], current: &[(&str, &[u8], bool)]| {
                entry_restoration_prompt(
                    "project",
                    Path::new("/project"),
                    proposal,
                    restored,
                    current,
                )
            };
        assert!(prompt(&proposal, &[], &files).is_err());
        assert!(prompt(&proposal, &files, &[]).is_err());
        for field in ["installed_tree", "schema", "project", "automatic_replay"] {
            let mut changed = proposal.clone();
            if let Json::Object(fields) = &mut changed {
                fields.iter_mut().find(|(key, _)| key == field).unwrap().1 = Json::Null;
            }
            assert!(prompt(&changed, &files, &files).is_err());
        }
        let absent = restoration_receipt(restoration_tree(false, b"text"), Json::Null);
        assert!(prompt(&absent, &files, &files).is_err());
        for bytes in [vec![0], vec![0xff], vec![b'x'; MAX_PROMPT]] {
            let receipt = restoration_receipt(restoration_tree(false, &bytes), Json::Null);
            assert!(prompt(&receipt, &[("", &bytes, false)], &[]).is_err());
        }
        let changed_install = restoration_receipt(restoration_tree(false, b"text"), Json::Null);
        let mut changed_install = changed_install;
        if let Json::Object(fields) = &mut changed_install {
            fields
                .iter_mut()
                .find(|(key, _)| key == "installed_tree")
                .unwrap()
                .1 = restoration_tree(false, b"other");
        }
        assert!(prompt(&changed_install, &files, &[]).is_err());
    }
}
