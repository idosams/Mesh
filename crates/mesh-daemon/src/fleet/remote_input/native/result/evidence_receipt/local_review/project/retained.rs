//! Resolve project actions from exact coordinator-retained identities, never renderer paths.
use super::*;

/// Native project action selection. No worker-supplied location or input manifest is accepted.
pub struct RetainedRemoteProjectRequest<'a> {
    /// Digest of the exact authenticated offer already received by this coordinator.
    pub offer: RecordDigest,
    /// Exact remote/local correlation selected for review.
    pub correlation: RecordDigest,
    /// Independently admitted original project.
    pub source: &'a ProvisionedAttachment,
    /// Native history trust configuration.
    pub reviewers: &'a TrustedReviewers,
    /// Stable native candidate/import retry identity.
    pub request: &'a str,
    /// Observed original-project main; no automatic rebasing is performed.
    pub expected_main: Option<&'a str>,
}
impl Runtime {
    fn with_retained_remote_project<T>(
        &mut self,
        request: &RetainedRemoteProjectRequest<'_>,
        action: impl FnOnce(
            &NativeRemoteResultReceiver<'_>,
            &mut Runtime,
            &RemoteProjectCandidateRequest<'_>,
        ) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let correlation = self
            .retained_remote_local_review(request.offer)?
            .filter(|value| value.digest() == request.correlation)
            .ok_or_else(refused)?;
        let location = self
            .remote_review_location(&correlation)?
            .ok_or_else(refused)?;
        let binding = Json::parse(&location).map_err(|_| refused())?;
        if binding.encode() != location {
            return Err(refused());
        }
        let destination =
            RemoteInputDestination::from_history_binding(&binding).map_err(store_error)?;
        // The content record was created only after independent peer authentication. Its exact
        // hash is bound by the committed local correlation; this is not trust inferred from peer bytes.
        let events = self
            .store
            .events(&format!("result-content-{}", correlation.content), 0, 2)?;
        let [event] = events.as_slice() else {
            return Err(refused());
        };
        if event.revision != 1
            || event.request != "content"
            || event.payload.len() > 65_536
            || hash(&event.payload) != correlation.content
        {
            return Err(refused());
        }
        let body = Json::parse(&event.payload).map_err(|_| refused())?;
        let offer = text(&body, "offer")?;
        if hash(offer) != request.offer {
            return Err(refused());
        }
        let signed = Json::parse(offer).map_err(|_| refused())?;
        let target = signed
            .get("body")
            .and_then(|v| v.get("target"))
            .ok_or_else(refused)?;
        let public = |name| {
            RecordDigest::parse_hex(text(target, name)?)
                .map(|v| PublicKey::from_bytes(*v.as_bytes()))
                .map_err(|_| refused())
        };
        let coordinator = public("coordinator")?;
        let worker = public("worker")?;
        let (receiver, content) = NativeRemoteResultReceiver::reopen_content_receipt(
            &destination,
            offer,
            crate::fleet::RemoteWorkerStatusRequest {
                runtime: self,
                lane: &correlation.lane,
                run: &correlation.run,
                coordinator,
                worker,
            },
        )?;
        if content.digest() != correlation.content {
            return Err(refused());
        }
        let lane = self
            .state()
            .lanes
            .get(&correlation.lane)
            .ok_or_else(refused)?;
        if lane.parent.is_some() || lane.source_project.as_deref() != Some(request.source.id()) {
            return Err(refused());
        }
        let input = request
            .source
            .prepare_remote_input(&lane.base.to_string())
            .map_err(store_error)?;
        let native = RemoteProjectCandidateRequest {
            input: input.manifest(),
            correlation: &correlation,
            source: request.source,
            reviewers: request.reviewers,
            request: request.request,
            expected_main: request.expected_main,
        };
        let result = action(&receiver, self, &native)?;
        input.verify_roots().map_err(store_error)?;
        if self.retained_remote_local_review(request.offer)?.as_ref() != Some(&correlation)
            || self.remote_review_location(&correlation)?.as_deref() != Some(&location)
            || destination.history_binding().map_err(store_error)? != binding
        {
            return Err(refused());
        }
        Ok(result)
    }
    /// Stage an exact received result privately; current eligibility is required.
    pub fn stage_retained_remote_project(
        &mut self,
        request: &RetainedRemoteProjectRequest<'_>,
    ) -> Result<Json, Error> {
        self.with_retained_remote_project(request, |receiver, runtime, native| {
            receiver.stage_project_candidate(runtime, native)
        })
    }
    /// Inspect durable import truth without signing, adopting execution or repairing state.
    pub fn inspect_retained_remote_project_import(
        &mut self,
        request: &RetainedRemoteProjectRequest<'_>,
        actor: PublicKey,
    ) -> Result<Json, Error> {
        self.with_retained_remote_project(request, |receiver, runtime, native| {
            receiver.inspect_project_candidate_import(runtime, native, actor)
        })
    }
    /// Append an exact private project version under the existing native signing/recovery boundary.
    pub fn import_retained_remote_project(
        &mut self,
        request: &RetainedRemoteProjectRequest<'_>,
        signer: &dyn crate::fleet::CandidateImportSigner,
    ) -> Result<Json, Error> {
        self.with_retained_remote_project(request, |receiver, runtime, native| {
            receiver.import_project_candidate(runtime, native, signer)
        })
    }
    /// Inspect or create the actual imported project review; this grants no protected-main approval.
    pub fn review_retained_remote_project_import(
        &mut self,
        request: &RetainedRemoteProjectRequest<'_>,
        create: bool,
    ) -> Result<Json, Error> {
        self.with_retained_remote_project(request, |receiver, runtime, native| {
            receiver.review_imported_project_candidate(runtime, native, create)
        })
    }
}
