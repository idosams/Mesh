//! Exact native presentation of retained remote results, independent of a live worker.
use super::*;

impl RemoteLocalReviewReceipt {
    /// Public immutable selector facts; native destination paths and credentials stay private.
    pub fn selection(&self) -> Json {
        Json::object([
            ("offer", Json::text(self.offer.to_string())),
            ("correlation", Json::text(self.digest().to_string())),
            ("lane", Json::text(&self.lane)),
            ("run", Json::text(&self.run)),
            ("version", Json::text(self.version.to_string())),
            ("bundle", Json::text(self.review.to_string())),
            (
                "remote_version",
                Json::text(self.remote_version.to_string()),
            ),
        ])
    }

    pub(super) fn with_review<T>(
        &self,
        destination: &RemoteInputDestination,
        manifest: &RemoteInputManifest,
        reviewers: &crate::TrustedReviewers,
        read: impl FnOnce(&crate::workspace::OpenWorkspace) -> io::Result<T>,
    ) -> io::Result<T> {
        if manifest.input() != self.remote_version || manifest.bundle() != self.remote_manifest {
            return Err(io::Error::other("remote result manifest changed"));
        }
        let (open, source) = destination.reopen_result_review_history(
            &self.allocation,
            self.mapping,
            self.evidence,
            manifest,
            reviewers,
            Some((self.version, self.review)),
        )?;
        source.verify_roots()?;
        let result = read(&open)?;
        source.verify_roots()?;
        // Recheck retained metadata and exact review after reading, without repair or adoption.
        self.reopen(destination, manifest, reviewers)?;
        Ok(result)
    }

    /// Present the native recorded review of the exact local copy. This is a saved result tree,
    /// not a comparison to the original project or an assertion of current import eligibility.
    pub fn saved_review(
        &self,
        destination: &RemoteInputDestination,
        manifest: &RemoteInputManifest,
        reviewers: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_review(destination, manifest, reviewers, |open| {
            let review = open
                .recorded_review_item(self.review)
                .ok_or_else(|| io::Error::other("retained remote review unavailable"))?;
            Ok(Json::object([
                ("schema", Json::text("mesh.remote-saved-review/v1")),
                ("objective", Json::text(&self.objective)),
                ("lane", Json::text(&self.lane)),
                ("run", Json::text(&self.run)),
                ("offer", Json::text(self.offer.to_string())),
                ("correlation", Json::text(self.digest().to_string())),
                ("version", Json::text(self.version.to_string())),
                ("bundle", Json::text(self.review.to_string())),
                (
                    "remote_version",
                    Json::text(self.remote_version.to_string()),
                ),
                ("comparison_basis", Json::text("received-result-tree")),
                ("approval_authority", Json::Bool(false)),
                ("review", review),
            ]))
        })
    }

    /// Read a displayed saved artifact by exact object identity and side. A filesystem path,
    /// unknown object, invalid side or unavailable immutable bytes cannot select live content.
    pub fn saved_review_artifact(
        &self,
        destination: &RemoteInputDestination,
        manifest: &RemoteInputManifest,
        reviewers: &crate::TrustedReviewers,
        selection: (&str, &str),
    ) -> io::Result<crate::ReviewArtifact> {
        let object = mesh_materializer::ObjectId::parse(selection.0)
            .map_err(|_| io::Error::other("remote review object invalid"))?;
        let side = match selection.1 {
            "before" => crate::workspace::ReviewArtifactSide::Before,
            "after" => crate::workspace::ReviewArtifactSide::After,
            _ => return Err(io::Error::other("remote review side invalid")),
        };
        self.with_review(destination, manifest, reviewers, |open| {
            open.verified_review_artifact(self.review, self.version, object, side)
                .map(crate::ReviewArtifact::from_verified)
                .map_err(io::Error::other)
        })
    }
}
