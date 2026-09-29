//! Durable discovery index. Original per-checkpoint signed records remain authoritative.
use super::*;
const MAX_RESULTS: u64 = 4096;
const PAGE_SIZE: usize = 16;
fn stream(body: &Json) -> Result<String, Error> {
    let scope = Json::object([
        ("target", body.get("target").ok_or_else(invalid)?.clone()),
        ("owner", Json::text(text(body, "owner")?)),
    ]);
    Ok(format!(
        "remote-result-catalog-{}",
        Blake3::digest_bytes(scope.encode().as_bytes())
    ))
}
fn existing(
    store: &FleetStore,
    stream: &str,
    offer: &RemoteSavedResultOffer,
) -> Result<bool, Error> {
    let Some(event) = store.request(stream, text(&offer.body, "checkpoint")?)? else {
        return Ok(false);
    };
    if event.revision == 0 || event.revision > MAX_RESULTS || event.payload != offer.encode() {
        return Err(invalid());
    }
    Ok(true)
}
pub(super) fn retain(
    store: &mut FleetStore,
    offer: RemoteSavedResultOffer,
) -> Result<RemoteSavedResultOffer, Error> {
    let stream = stream(&offer.body)?;
    if existing(store, &stream, &offer)? {
        return Ok(offer);
    }
    let revision = store.revision(&stream)?;
    if revision >= MAX_RESULTS {
        return Err(invalid());
    }
    let result = store.append_with_outcome(
        &stream,
        revision,
        text(&offer.body, "checkpoint")?,
        &offer.encode(),
    );
    // A competing exact append may have won. Never retry an unrelated stale revision blindly.
    if existing(store, &stream, &offer)? {
        Ok(offer)
    } else {
        result?;
        Err(invalid())
    }
}

/// Native discovery page for one original launch. This is not authenticated transport or proof
/// of complete content. Pre-catalog offers are discoverable only after explicit republication.
pub struct RemoteSavedResultPage {
    /// Durable catalog revision observed before reading this page; later appends are not included.
    pub revision: u64,
    /// Cursor after the returned rows. Reusing it discovers later appended results, including at end.
    pub after: u64,
    /// Whether more rows existed at the observed catalog revision.
    pub has_more: bool,
    /// At most sixteen validated original signed offers in durable publication order.
    pub offers: Vec<RemoteSavedResultOffer>,
}
impl RemoteAdmissionRegistry {
    /// Read a bounded catalog page without restoring a provider or touching its working files.
    /// Unknown launch returns None. A cursor beyond the observed catalog revision refuses.
    /// This catalog includes offers published or explicitly republished since catalog support;
    /// older offers remain readable through the known-checkpoint API. Empty is not completion.
    pub fn saved_result_page(
        &self,
        assignment: &str,
        after: u64,
    ) -> Result<Option<RemoteSavedResultPage>, Error> {
        id_valid(assignment)?;
        if !self
            .receipts()?
            .iter()
            .any(|r| r.work().assignment.id == assignment)
        {
            return Ok(None);
        }
        let Some(launch) = self.launch_receipt(assignment)? else {
            return Ok(None);
        };
        let admission = launch.admission();
        let scope = Json::object([
            (
                "target",
                identity(
                    admission.coordinator(),
                    admission.objective(),
                    admission.work(),
                ),
            ),
            ("owner", Json::text(launch.owner())),
        ]);
        let stream = stream(&scope)?;
        let revision = self.store.revision(&stream)?;
        if revision > MAX_RESULTS || after > revision {
            return Err(invalid());
        }
        let count = (revision - after).min(PAGE_SIZE as u64) as usize;
        let mut cursor = after;
        let mut offers = Vec::with_capacity(count);
        if count > 0 {
            for event in self.store.events(&stream, after, count)? {
                cursor += 1;
                if event.revision != cursor {
                    return Err(invalid());
                }
                let indexed = RemoteSavedResultOffer::decode(&event.payload)?;
                if text(&indexed.body, "checkpoint")? != event.request {
                    return Err(invalid());
                }
                let original = self
                    .saved_result_offer(assignment, &event.request)?
                    .ok_or_else(invalid)?;
                if original.encode() != event.payload {
                    return Err(invalid());
                }
                offers.push(original);
            }
        }
        if offers.len() != count || self.launch_receipt(assignment)?.as_ref() != Some(&launch) {
            return Err(invalid());
        }
        Ok(Some(RemoteSavedResultPage {
            revision,
            after: cursor,
            has_more: cursor < revision,
            offers,
        }))
    }
}

#[cfg(test)]
#[path = "catalog/tests.rs"]
mod tests;
