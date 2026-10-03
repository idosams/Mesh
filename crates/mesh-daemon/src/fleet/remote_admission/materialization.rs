//! Durable identity of acknowledged input; never permission to allocate or launch again.
use super::*;
use crate::fleet::RemoteInputAllocation;

/// Exact native allocation recorded before a receiving handoff may be acknowledged.
/// Retained identity is historical evidence, not proof of current contents or execution.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteMaterializationReceipt {
    admission: RemoteAdmissionReceipt,
    roots: [String; 3],
}
impl RemoteMaterializationReceipt {
    /// Original immutable assignment and allocation name.
    pub fn admission(&self) -> &RemoteAdmissionReceipt {
        &self.admission
    }
    /// Recorded physical parent, allocation and files identities, respectively.
    /// Native recovery must independently reopen and verify every identity before use.
    pub fn directory_identities(&self) -> &[String; 3] {
        &self.roots
    }
}
impl RemoteAdmissionRegistry {
    pub(in crate::fleet) fn retain_materialization(
        &mut self,
        allocation: &RemoteInputAllocation,
    ) -> Result<RemoteMaterializationReceipt, Error> {
        let admission = allocation
            .admission
            .as_ref()
            .ok_or(Error::InvalidHistory)?
            .clone();
        if self.receipts()?.iter().all(|a| a != &admission)
            || allocation.manifest().input() != admission.work.assignment.input
            || allocation.manifest().bundle() != admission.work.assignment.bundle
        {
            return refuse("remote-materialization-admission-mismatch");
        }
        let roots = allocation
            .retained_identity()
            .map_err(|_| Error::Refused("remote-materialization-storage-changed"))?;
        let proposed = RemoteMaterializationReceipt { admission, roots };
        let payload = self.encode_materialization(&proposed);
        let event = self
            .store
            .append_with_outcome(
                &self.materialization_stream(&proposed.admission.work.assignment.id),
                0,
                "materialized",
                &payload,
            )?
            .into_event();
        let retained = self.decode_materialization(&proposed.admission, &event)?;
        if retained != proposed
            || allocation.retained_identity().ok().as_ref() != Some(&proposed.roots)
        {
            return refuse("remote-materialization-storage-changed");
        }
        Ok(retained)
    }
    /// Read one immutable receipt without opening storage, allocating, renewing or launching.
    /// None means no receipt was committed, not that materialization never happened.
    pub fn materialization_receipt(
        &self,
        assignment: &str,
    ) -> Result<Option<RemoteMaterializationReceipt>, Error> {
        let admission = self
            .receipts()?
            .into_iter()
            .find(|a| a.work.assignment.id == assignment)
            .ok_or(Error::Refused("remote-materialization-admission-missing"))?;
        let events = self
            .store
            .events(&self.materialization_stream(assignment), 0, 2)?;
        match events.as_slice() {
            [] => Ok(None),
            [event] => self.decode_materialization(&admission, event).map(Some),
            _ => Err(Error::InvalidHistory),
        }
    }
    fn materialization_stream(&self, assignment: &str) -> String {
        let key = Json::object([
            ("admissions", Json::text(&self.stream)),
            ("assignment", Json::text(assignment)),
        ])
        .encode();
        format!(
            "remote-materialization-{}",
            Blake3::digest_bytes(key.as_bytes())
        )
    }
    fn encode_materialization(&self, receipt: &RemoteMaterializationReceipt) -> String {
        let admission = self.encode(&receipt.admission.work, &receipt.admission.allocation);
        Json::object([
            ("schema", Json::text("mesh.remote-materialization/v1")),
            (
                "admission",
                Json::text(Blake3::digest_bytes(admission.as_bytes()).to_string()),
            ),
            (
                "admission_revision",
                Json::Number(receipt.admission.revision),
            ),
            ("parent", Json::text(&receipt.roots[0])),
            ("allocation", Json::text(&receipt.roots[1])),
            ("files", Json::text(&receipt.roots[2])),
        ])
        .encode()
    }
    fn decode_materialization(
        &self,
        admission: &RemoteAdmissionReceipt,
        event: &FleetEvent,
    ) -> Result<RemoteMaterializationReceipt, Error> {
        let value = Json::parse(&event.payload).map_err(|_| Error::InvalidHistory)?;
        let identity = |key| -> Result<String, Error> {
            let text = value
                .get(key)
                .and_then(Json::as_text)
                .ok_or(Error::InvalidHistory)?;
            let token = crate::ProtectedWorkspaceRoot::from_directory_token(text)
                .map_err(|_| Error::InvalidHistory)?;
            if token.directory_token() != text {
                return Err(Error::InvalidHistory);
            }
            Ok(text.into())
        };
        let receipt = RemoteMaterializationReceipt {
            admission: admission.clone(),
            roots: [
                identity("parent")?,
                identity("allocation")?,
                identity("files")?,
            ],
        };
        if receipt.roots[0] == receipt.roots[1]
            || receipt.roots[0] == receipt.roots[2]
            || receipt.roots[1] == receipt.roots[2]
            || event.stream != self.materialization_stream(&admission.work.assignment.id)
            || event.revision != 1
            || event.request != "materialized"
            || self.encode_materialization(&receipt) != event.payload
        {
            return Err(Error::InvalidHistory);
        }
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::remote_admission::authentication::tests::Fixture;
    #[test]
    fn materialization_reader_rejects_wrong_identity_context_and_noncanonical_records() {
        let fixture = Fixture::new();
        let mut registry = fixture.registry();
        let RemoteAdmissionOutcome::Reserved(reservation) = registry
            .reserve(
                fixture.work.clone(),
                "0123456789abcdef0123456789abcdef",
                crate::fleet::service::received_clock().unwrap(),
            )
            .unwrap()
        else {
            panic!("fresh admission")
        };
        let receipt = RemoteMaterializationReceipt {
            admission: reservation.receipt().clone(),
            roots: [
                "0000000000000001:0000000000000001".into(),
                "0000000000000001:0000000000000002".into(),
                "0000000000000001:0000000000000003".into(),
            ],
        };
        let event = FleetEvent {
            stream: registry.materialization_stream("assignment"),
            revision: 1,
            request: "materialized".into(),
            payload: registry.encode_materialization(&receipt),
        };
        assert!(
            registry
                .decode_materialization(&receipt.admission, &event)
                .unwrap()
                == receipt
        );
        for change in 0..7 {
            let mut bad = event.clone();
            match change {
                0 => bad.stream.push('x'),
                1 => bad.revision = 2,
                2 => bad.request = "other".into(),
                3 => bad.payload.push(' '),
                4 => {
                    bad.payload = bad
                        .payload
                        .replace("materialization/v1", "materialization/v2")
                }
                5 => {
                    bad.payload = bad
                        .payload
                        .replace("0000000000000001:0000000000000003", "invalid")
                }
                _ => {
                    bad.payload = bad.payload.replace(
                        "0000000000000001:0000000000000003",
                        "0000000000000001:0000000000000002",
                    )
                }
            }
            assert!(registry
                .decode_materialization(&receipt.admission, &bad)
                .is_err());
        }
        let mut changed = receipt.admission.clone();
        changed.work.assignment.bundle = RecordDigest::from_bytes([8; 32]);
        assert!(registry.decode_materialization(&changed, &event).is_err());
        registry
            .store
            .append(&event.stream, 0, &event.request, &event.payload)
            .unwrap();
        assert!(
            fixture
                .registry()
                .materialization_receipt("assignment")
                .unwrap()
                .as_ref()
                == Some(&receipt)
        );
        registry
            .store
            .append(&event.stream, 1, "unexpected-extra", &event.payload)
            .unwrap();
        assert!(fixture
            .registry()
            .materialization_receipt("assignment")
            .is_err());
    }
}
