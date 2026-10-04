//! A verified consumed history may participate in graph inspection under the same custody set.
use super::*;
use crate::project_attachment::{
    dependency_owner_context::{OwnerHistoryContext, VerifiedHistoryRoots},
    dependency_transaction::{digest, read_payload, text},
    NativeDependencyGraph,
};
use crate::{root_authority::PinnedRootFs, workspace_custody::WorkspaceInitializationGuard};
use mesh_cas::{Blake3, Cas};
impl PreparedNativeConsumedStart {
    pub(super) fn inspect_completed_graph(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
        source_graph: &NativeDependencyGraph,
        guard: &WorkspaceInitializationGuard,
        version: SavedAttachmentVersion,
    ) -> io::Result<NativeDependencyGraph> {
        let (configuration, proof) = self.verify_completed_history(source_graph, guard, None)?;
        let roots = self.completed_graph_roots(staged, source_graph)?;
        let available = self
            .available
            .iter()
            .chain(std::iter::once(&self.source))
            .collect::<Vec<_>>();
        let context = storage
            .resolve_consumed_histories(
                &self.owner,
                &available,
                guard,
                OwnerHistoryContext::current(&self.owner),
            )?
            .with_verified_history(
                &self.destination,
                configuration.clone(),
                proof.clone(),
                roots,
            )?;
        let selected = storage.prepare_dependency_graph(
            &self.owner,
            &self.destination,
            version.operation(),
            &available,
        )?;
        // Preparation cannot extend the caller's already-held lock set.
        guard.require_roots(&selected.roots).map_err(error)?;
        let result = storage.inspect_dependency_graph_with_owner(&selected, guard, &context)?;
        let (after_configuration, after) =
            self.verify_completed_history(source_graph, guard, None)?;
        if after_configuration != configuration || after != proof {
            return Err(invalid("consumed graph history changed"));
        }
        context.verified_history_roots(&self.destination)?;
        Ok(result)
    }
    pub(super) fn completed_graph_roots(
        &self,
        staged: &StagedNativeConsumedStart,
        source_graph: &NativeDependencyGraph,
    ) -> io::Result<VerifiedHistoryRoots> {
        let mut roots = VerifiedHistoryRoots::default();
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.destination.metadata_path(),
            self.destination.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let raw = read_private_in_store(&self.destination.store, super::super::START_PENDING)?;
        let start_intent = Json::parse(&raw).map_err(error)?;
        let payload = read_payload(&cas, digest(text(&start_intent, "payload")?)?, 65536)?;
        let start = Json::parse(std::str::from_utf8(&payload).map_err(error)?).map_err(error)?;
        let body = start
            .get("body")
            .ok_or_else(|| invalid("consumed graph start missing"))?;
        let descriptor = digest(text(body, "staged")?)?;
        let descriptor_bytes = read_payload(&cas, descriptor, 4096)?;
        let decoded =
            Json::parse(std::str::from_utf8(&descriptor_bytes).map_err(error)?).map_err(error)?;
        if digest(text(&decoded, "stage")?)? != staged.receipt {
            return Err(invalid("consumed graph stage changed"));
        }
        let stage = read_payload(&cas, staged.receipt, MAX_RECEIPT)?;
        if stage != read(&staged.root, RECEIPT, MAX_RECEIPT)? {
            return Err(invalid("consumed graph stage object differs"));
        }
        for bytes in self.checkpoint.objects.iter().map(Vec::as_slice).chain([
            payload.as_slice(),
            descriptor_bytes.as_slice(),
            stage.as_slice(),
            self.basis.configuration.as_bytes(),
            self.basis.prospective.as_bytes(),
        ]) {
            let id = hash(bytes);
            if read_payload(&cas, id, bytes.len())? != bytes {
                return Err(invalid("consumed graph retained object changed"));
            }
            roots.payloads.insert(id);
        }
        for bytes in [self.frames(), source_graph.to_json().encode().into_bytes()] {
            let id = hash(&bytes);
            if read_payload(&cas, id, bytes.len())? != bytes {
                return Err(invalid("consumed graph retained frames or closure changed"));
            }
            roots.payloads.insert(id);
        }
        let expected_owner = Json::object([
            (
                "schema",
                Json::text("mesh.native-consumption-owner-intent/v1"),
            ),
            ("start", Json::text(hash(&payload).to_hex())),
            ("closure", Json::text(source_graph.digest().to_hex())),
            (
                "body",
                Json::object([
                    ("request", Json::text(self.request.to_hex())),
                    ("grant", Json::text(self.grant.to_hex())),
                    (
                        "start",
                        Json::Array(vec![
                            Json::Array(vec![
                                Json::text(self.basis.destination.work().to_hex()),
                                Json::text(self.basis.destination.installation().to_hex()),
                            ]),
                            Json::text(self.operation().to_hex()),
                        ]),
                    ),
                    ("inputs", source_graph.consumption_inputs_json()),
                ]),
            ),
        ])
        .encode();
        if read_private_in_store(&self.destination.store, super::installation::OWNER_INTENT)?
            != expected_owner
        {
            return Err(invalid("consumed graph owner intent changed"));
        }
        for name in [
            super::super::START_PENDING,
            crate::project_attachment::consumption_history::PENDING,
            crate::project_attachment::consumption_complete::PENDING,
            super::installation::OWNER_INTENT,
        ] {
            let raw = read_private_in_store(&self.destination.store, name)?;
            roots.sidecars.insert(name.to_owned(), hash(raw.as_bytes()));
        }
        // Owner receipt and source/grant references live in other native stores. Their payloads
        // are retained by those graph entries, never guessed to be local CAS content here.
        Ok(roots)
    }
}

#[cfg(test)]
impl PreparedNativeConsumedStart {
    pub(super) fn assert_graph_context_revalidation(
        &self,
        graph: &NativeDependencyGraph,
        guard: &WorkspaceInitializationGuard,
    ) {
        let (configuration, proof) = self.verify_completed_history(graph, guard, None).unwrap();
        let context = OwnerHistoryContext::current(&self.owner)
            .with_verified_history(
                &self.destination,
                configuration,
                proof,
                VerifiedHistoryRoots::default(),
            )
            .unwrap();
        let marker = self
            .destination
            .metadata_path()
            .join(crate::project_attachment::history::HISTORY);
        let original = fs::read_to_string(&marker).unwrap();
        let mut value = Json::parse(&original).unwrap();
        let Json::Object(fields) = &mut value else {
            panic!("marker must be an object")
        };
        fields
            .iter_mut()
            .find(|(name, _)| name == "previous_binding")
            .unwrap()
            .1 = Json::text(self.basis.prospective.clone());
        assert_ne!(value.encode(), original);
        fs::write(&marker, value.encode()).unwrap();
        assert!(
            context.read(&self.destination).is_err(),
            "changed configuration reused a graph proof"
        );
        fs::write(&marker, &original).unwrap();
        assert!(context.read(&self.destination).is_ok());
    }
}
