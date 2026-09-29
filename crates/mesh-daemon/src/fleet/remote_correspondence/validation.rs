use super::super::RemoteInputEntry;
use super::*;
use std::collections::BTreeSet;
fn invalid() -> io::Error {
    io::Error::other("result correspondence is not exact canonical evidence")
}
fn closed(value: &Json, names: &[&str]) -> io::Result<Json> {
    let Json::Object(fields) = value else {
        return Err(invalid());
    };
    if fields.len() != names.len()
        || fields.iter().map(|(k, _)| k).collect::<BTreeSet<_>>().len() != fields.len()
    {
        return Err(invalid());
    }
    Ok(Json::Object(
        names
            .iter()
            .map(|name| Ok(((*name).into(), value.get(name).ok_or_else(invalid)?.clone())))
            .collect::<io::Result<_>>()?,
    ))
}
fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    value.get(key).and_then(Json::as_text).ok_or_else(invalid)
}
fn entries(manifest: &RemoteInputManifest) -> BTreeMap<&str, bool> {
    manifest
        .entries()
        .iter()
        .map(|entry| match entry {
            RemoteInputEntry::Directory { path } => (path.as_str(), true),
            RemoteInputEntry::File { path, .. } => (path.as_str(), false),
        })
        .collect()
}
impl RemoteResultCorrespondence {
    pub(in crate::fleet) fn decode_bound(
        raw: &str,
        input: &RemoteInputManifest,
        result: &RemoteInputManifest,
        initial: RecordDigest,
    ) -> io::Result<Self> {
        if raw.len() > 4_194_304 {
            return Err(invalid());
        }
        let parsed = Json::parse(raw).map_err(|_| invalid())?;
        let body = closed(
            &parsed,
            &[
                "schema",
                "input",
                "input_manifest",
                "worker_initial",
                "result",
                "result_manifest",
                "entries",
            ],
        )?;
        if text(&body, "schema")? != "mesh.remote-result-correspondence/v1"
            || text(&body, "input")? != input.input().to_string()
            || text(&body, "input_manifest")? != input.bundle().to_string()
            || text(&body, "worker_initial")? != initial.to_string()
            || text(&body, "result")? != result.input().to_string()
            || text(&body, "result_manifest")? != result.bundle().to_string()
        {
            return Err(invalid());
        }
        let original = entries(input);
        let mut expected = entries(result);
        let rows = body
            .get("entries")
            .and_then(Json::as_array)
            .ok_or_else(invalid)?;
        if rows.len() != expected.len() {
            return Err(invalid());
        }
        let mut objects = BTreeSet::new();
        let mut origins = BTreeSet::new();
        let mut previous = None;
        for row in rows {
            let canonical = closed(row, &["path", "object", "kind", "input_path"])?;
            if canonical != *row {
                return Err(invalid());
            }
            let path = text(row, "path")?;
            if previous.is_some_and(|last| last >= path) {
                return Err(invalid());
            }
            previous = Some(path);
            let directory = expected.remove(path).ok_or_else(invalid)?;
            if text(row, "kind")? != if directory { "directory" } else { "file" } {
                return Err(invalid());
            }
            let object = text(row, "object")?;
            if object.len() != 32
                || !object
                    .bytes()
                    .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
                || !objects.insert(object)
            {
                return Err(invalid());
            }
            match row.get("input_path").ok_or_else(invalid)? {
                Json::Null => (),
                Json::Text(source)
                    if original.get(source.as_str()) == Some(&directory)
                        && origins.insert(source) => {}
                _ => return Err(invalid()),
            }
        }
        if body.encode() != raw {
            return Err(invalid());
        }
        let digest = RecordDigest::from_bytes(*Blake3::digest_bytes(raw.as_bytes()).as_bytes());
        Ok(Self {
            encoded: raw.into(),
            digest,
        })
    }
}
