use super::*;
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "mesh-start-journal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
    fn open(&self) -> AttachmentStorage {
        AttachmentStorage::open(&self.0).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn request(n: usize) -> RemoteStartRequest {
    RemoteStartRequest {
        request: format!("{n:032x}"),
        value: Json::object([
            ("peer", Json::text("worker.example")),
            ("lease", Json::Number(1234)),
        ]),
    }
}
#[test]
fn original_inputs_survive_restart_and_refuse_changed_retry() {
    let f = Fixture::new();
    let s = f.open();
    assert!(s.remote_start_requests().unwrap().is_empty());
    let original = request(1);
    s.retain_remote_start_request(&original).unwrap();
    s.retain_remote_start_request(&original).unwrap();
    drop(s);
    let s = f.open();
    assert_eq!(s.remote_start_requests().unwrap(), vec![original.clone()]);
    assert_eq!(
        fs::metadata(f.0.join(original.name())).unwrap().mode() & 0o777,
        0o600
    );
    for field in ["peer", "lease"] {
        let mut changed = original.clone();
        let Json::Object(fields) = &mut changed.value else {
            unreachable!()
        };
        fields.iter_mut().find(|(k, _)| k == field).unwrap().1 = Json::text("changed");
        assert!(s.retain_remote_start_request(&changed).is_err());
        assert_eq!(s.remote_start_requests().unwrap(), vec![original.clone()]);
    }
    assert!(s.registrations().unwrap().is_empty());
}
#[test]
fn partial_public_linked_oversized_and_copied_records_refuse_without_repair() {
    for mode in [
        "partial",
        "public",
        "hardlink",
        "symlink",
        "oversized",
        "copied",
        "renamed",
    ] {
        let f = Fixture::new();
        let other = Fixture::new();
        let s = f.open();
        let input = request(1);
        s.retain_remote_start_request(&input).unwrap();
        let path = f.0.join(input.name());
        match mode {
            "partial" => fs::write(&path, b"{").unwrap(),
            "public" => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            "hardlink" => fs::hard_link(&path, f.0.join("retained")).unwrap(),
            "symlink" => {
                fs::rename(&path, f.0.join("retained")).unwrap();
                std::os::unix::fs::symlink(f.0.join("retained"), &path).unwrap();
            }
            "oversized" => fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap(),
            "copied" => {
                fs::copy(&path, other.0.join(input.name())).unwrap();
                assert!(other.open().remote_start_requests().is_err());
                continue;
            }
            "renamed" => fs::rename(&path, f.0.join(request(2).name())).unwrap(),
            _ => unreachable!(),
        }
        let before = fs::read_dir(&f.0).unwrap().count();
        assert!(s.remote_start_requests().is_err(), "{mode}");
        assert!(
            s.retain_remote_start_request(&request(3)).is_err(),
            "{mode}"
        );
        assert_eq!(fs::read_dir(&f.0).unwrap().count(), before);
    }
}
#[test]
fn bounds_refuse_new_requests_but_preserve_exact_replay_at_capacity() {
    let f = Fixture::new();
    let s = f.open();
    for bad in [
        RemoteStartRequest {
            request: "../outside".into(),
            ..request(1)
        },
        RemoteStartRequest {
            value: Json::Null,
            ..request(1)
        },
        RemoteStartRequest {
            value: Json::object([("large", Json::text("x".repeat(24_576)))]),
            ..request(1)
        },
    ] {
        assert!(s.retain_remote_start_request(&bad).is_err());
    }
    assert!(s.remote_start_requests().unwrap().is_empty());
    for n in 0..MAX_REQUESTS {
        s.retain_remote_start_request(&request(n)).unwrap();
    }
    s.retain_remote_start_request(&request(0)).unwrap();
    assert!(s
        .retain_remote_start_request(&request(MAX_REQUESTS))
        .is_err());
    assert_eq!(s.remote_start_requests().unwrap().len(), MAX_REQUESTS);
}
