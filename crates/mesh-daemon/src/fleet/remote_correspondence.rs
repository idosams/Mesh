//! Native identity correspondence from exact original input to a recorded worker result.
use super::RemoteInputManifest;
use crate::ipc::Json;
use crate::workspace::HistoricalWorkspacePreview;
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _};
use std::collections::BTreeMap;
use std::io;

/// Bounded native-derived evidence for a worker result. These bytes still require authenticated
/// transport binding before a coordinator may use them. They are never an import/approval grant.
pub struct RemoteResultCorrespondence {
    encoded: String,
    digest: RecordDigest,
}
impl RemoteResultCorrespondence {
    /// Canonical private metadata, including paths and object correlation; do not log it.
    pub fn encoded(&self) -> &str {
        &self.encoded
    }
    /// Digest of the complete schema-bound evidence, for a separately authenticated envelope.
    pub fn digest(&self) -> RecordDigest {
        self.digest
    }
    pub(in crate::fleet) fn derive(
        input: &RemoteInputManifest,
        initial: &HistoricalWorkspacePreview,
        result: &RemoteInputManifest,
        saved: &HistoricalWorkspacePreview,
    ) -> io::Result<Self> {
        let invalid = || io::Error::other("saved result correspondence unavailable");
        if !input.matches_saved_content(initial)
            || !result.matches_saved_content(saved)
            || result.input() != saved.operation
        {
            return Err(invalid());
        }
        // Reuse the same complete identity rules as local project candidate preparation.
        let origins = super::project_mapping::import_correspondence(
            initial.clone(),
            vec![(initial.clone(), saved.clone())],
        )?;
        let entries = |snapshot: &HistoricalWorkspacePreview| -> BTreeMap<String, (String, bool)> {
            snapshot
                .directories
                .iter()
                .map(|v| (v.object.to_string(), (v.path.clone(), true)))
                .chain(
                    snapshot
                        .files
                        .iter()
                        .map(|v| (v.object.to_string(), (v.path.clone(), false))),
                )
                .collect()
        };
        let original = entries(initial);
        let mut rows = Vec::new();
        for (object, (path, directory)) in entries(saved) {
            let source = match origins.get(&object) {
                Some(origin) => {
                    let (source, source_directory) = original.get(origin).ok_or_else(invalid)?;
                    if *source_directory != directory {
                        return Err(invalid());
                    }
                    Json::text(source)
                }
                None => Json::Null,
            };
            rows.push((
                path.clone(),
                Json::object([
                    ("path", Json::text(path)),
                    ("object", Json::text(object)),
                    (
                        "kind",
                        Json::text(if directory { "directory" } else { "file" }),
                    ),
                    ("input_path", source),
                ]),
            ));
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let body = Json::object([
            ("schema", Json::text("mesh.remote-result-correspondence/v1")),
            ("input", Json::text(input.input().to_string())),
            ("input_manifest", Json::text(input.bundle().to_string())),
            ("worker_initial", Json::text(initial.operation.to_string())),
            ("result", Json::text(result.input().to_string())),
            ("result_manifest", Json::text(result.bundle().to_string())),
            (
                "entries",
                Json::Array(rows.into_iter().map(|(_, row)| row).collect()),
            ),
        ]);
        let encoded = body.encode();
        // Both source and result manifests are independently bounded to 1 MiB. Reserve a bounded
        // envelope for paired names and object identities; this is not a 64 KiB control frame.
        if encoded.len() > 4_194_304 {
            return Err(invalid());
        }
        let digest = RecordDigest::from_bytes(*Blake3::digest_bytes(encoded.as_bytes()).as_bytes());
        Ok(Self { encoded, digest })
    }
    /// Map an independently imported exact result to the original native objects. Only a caller
    /// holding authenticated, retained evidence may use this mapping for candidate preparation.
    pub(in crate::fleet) fn project_origins(
        &self,
        input: &RemoteInputManifest,
        original: &HistoricalWorkspacePreview,
        result: &RemoteInputManifest,
        local: &HistoricalWorkspacePreview,
    ) -> io::Result<BTreeMap<String, String>> {
        let invalid = || io::Error::other("remote project correspondence unavailable");
        if original.operation != input.input()
            || !input.matches_saved_content(original)
            || !result.matches_saved_content(local)
        {
            return Err(invalid());
        }
        let body = Json::parse(self.encoded()).map_err(|_| invalid())?;
        let initial = body
            .get("worker_initial")
            .and_then(Json::as_text)
            .and_then(|v| RecordDigest::parse_hex(v).ok())
            .ok_or_else(invalid)?;
        Self::decode_bound(self.encoded(), input, result, initial)?;
        let entries = |snapshot: &HistoricalWorkspacePreview| -> io::Result<BTreeMap<String, (String, bool)>> {
            let mut paths = BTreeMap::new();
            let mut objects = std::collections::BTreeSet::new();
            for (path, object, directory) in snapshot.directories.iter()
                .map(|v| (&v.path, v.object.to_string(), true))
                .chain(snapshot.files.iter().map(|v| (&v.path, v.object.to_string(), false))) {
                if !objects.insert(object.clone()) || paths.insert(path.clone(), (object, directory)).is_some() {
                    return Err(invalid());
                }
            }
            Ok(paths)
        };
        let original = entries(original)?;
        let local = entries(local)?;
        let mut origins = BTreeMap::new();
        for row in body
            .get("entries")
            .and_then(Json::as_array)
            .ok_or_else(invalid)?
        {
            let path = row
                .get("path")
                .and_then(Json::as_text)
                .ok_or_else(invalid)?;
            let target = local.get(path).ok_or_else(invalid)?;
            if let Some(source) = row.get("input_path").and_then(Json::as_text) {
                let source = original.get(source).ok_or_else(invalid)?;
                if source.1 != target.1 {
                    return Err(invalid());
                }
                origins.insert(target.0.clone(), source.0.clone());
            }
        }
        Ok(origins)
    }
}

#[cfg(test)]
mod tests;

mod validation;
