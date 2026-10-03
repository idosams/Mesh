//! Native-only bounded application discovery; the provider adapter separately admits execution.
use std::path::PathBuf;
pub(crate) fn find_cli(applications: &[PathBuf]) -> Option<PathBuf> {
    applications
        .iter()
        .flat_map(|root| {
            ["ChatGPT.app", "Codex.app"]
                .into_iter()
                .flat_map(move |app| {
                    [
                        "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
                        "Contents/Resources/codex",
                    ]
                    .into_iter()
                    .map(move |layout| root.join(app).join(layout))
                })
        })
        .find(|candidate| {
            std::fs::symlink_metadata(candidate).is_ok_and(|m| m.file_type().is_file())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    const NESTED: &str = "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex";
    const LEGACY: &str = "Contents/Resources/codex";
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "mesh-codex-discovery-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn file(&self, root: &str, app: &str, layout: &str) -> PathBuf {
            let path = self.0.join(root).join(app).join(layout);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"fixture, never executed").unwrap();
            path
        }
        fn roots(&self) -> Vec<PathBuf> {
            vec![self.0.join("system"), self.0.join("user")]
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn installed_nested_cli_is_discovered_in_each_supported_application_location() {
        for root in ["system", "user"] {
            for app in ["ChatGPT.app", "Codex.app"] {
                let fixture = Fixture::new();
                let expected = fixture.file(root, app, NESTED);
                assert_eq!(find_cli(&fixture.roots()), Some(expected));
            }
        }
    }
    #[test]
    fn legacy_layout_remains_supported_and_current_bundle_layout_takes_precedence() {
        let f = Fixture::new();
        let legacy = f.file("system", "ChatGPT.app", LEGACY);
        assert_eq!(find_cli(&f.roots()), Some(legacy));
        let nested = f.file("system", "ChatGPT.app", NESTED);
        assert_eq!(find_cli(&f.roots()), Some(nested));
    }
    #[test]
    fn absent_directory_and_link_candidates_do_not_become_executable_selections() {
        let f = Fixture::new();
        assert_eq!(find_cli(&f.roots()), None);
        let path = f.0.join("system/ChatGPT.app").join(NESTED);
        fs::create_dir_all(&path).unwrap();
        assert_eq!(find_cli(&f.roots()), None);
        fs::remove_dir(&path).unwrap();
        let target = f.file("other", "Unlisted.app", LEGACY);
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert_eq!(find_cli(&f.roots()), None);
        let fallback = f.file("user", "Codex.app", LEGACY);
        assert_eq!(find_cli(&f.roots()), Some(fallback));
    }
}
