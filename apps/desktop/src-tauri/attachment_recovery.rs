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
    let prompt = format!(
        "{action}\n\nProject: {project:?}\nFolder: {root:?}\nFile: {:?}\n\nCURRENT FILE TO PRESERVE\nDigest: {}\nPermissions: {:o}\n{}\n\nCONTENT TO INSTALL\nDigest: {}\nPermissions: {:o}\n{}\n\nThis changes only this working file. Its current file is retained in private recovery storage, including later writes through an already-open editor. Existing retained files stay available. Mesh main and Git are unchanged. Restored content remains private work.\n\nThe exact inputs will be checked again after confirmation. A concurrent change can require recovery inspection. Cancel leaves your working file unchanged and retains the prepared record.",
        field("path")?, field("source_digest")?, mode("source_mode")?, text(current)?,
        field("installed_digest")?, mode("installed_mode")?, text(proposed)?,
    );
    if prompt.len() > MAX_PROMPT {
        return Err("This change is too large to show completely in the native confirmation. No content will be omitted.".into());
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
}
