//! Immutable native receipt selections. Persistence never grants transfer or review authority.
use super::*;
use crate::fleet::RemoteSavedResultOffer;
const MAX_INTENTS: usize = 64;
const MAX_BYTES: u64 = 65_536;

/// Saved native selection, not received content. Re-admit its configuration, assignment and trust
/// before transport; use the retained allocation for the exact selected offer on explicit retry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteReceiptIntent {
    id: String,
    offer: String,
    context: Json,
}
impl RemoteReceiptIntent {
    /// Opaque exact-offer digest suitable for a display selection, never a path.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Stable native allocation identity. Changed offers cannot reuse this intent.
    pub fn allocation(&self) -> &str {
        &self.id[..32]
    }
    /// Original signed private offer. Do not expose this to a renderer or general logs.
    pub fn offer(&self) -> &str {
        &self.offer
    }
    /// Original native configuration/bindings. Re-admission is required before use.
    pub fn context(&self) -> &Json {
        &self.context
    }
    fn new(offer: &str, context: &Json) -> io::Result<Self> {
        RemoteSavedResultOffer::content_identity(offer).map_err(|_| invalid())?;
        if !matches!(context, Json::Object(_)) || context.encode().len() > 24_576 {
            return Err(invalid());
        }
        Ok(Self {
            id: Blake3::digest_bytes(offer.as_bytes()).to_string(),
            offer: offer.into(),
            context: context.clone(),
        })
    }
    fn record(&self, inbox: &RemoteResultInbox) -> io::Result<String> {
        let raw = Json::object([
            ("schema", Json::text("mesh.remote-receipt-intent/v1")),
            ("inbox", Json::text(token(&inbox.root)?.directory_token())),
            ("id", Json::text(&self.id)),
            ("allocation", Json::text(self.allocation())),
            ("offer", Json::text(&self.offer)),
            ("context", self.context.clone()),
        ])
        .encode();
        if raw.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        Ok(raw)
    }
    fn name(&self) -> String {
        format!("receipt-{}.json", self.id)
    }
}
impl RemoteResultInbox {
    /// Persist an exact native selection before network or materialization. Identical repetition
    /// returns the original intent; changed context for the same signed offer refuses. The native
    /// owner serializes mutations with a physical-root lock. No removal or automatic retry occurs.
    pub fn retain_receipt_intent(
        &self,
        offer: &RemoteSavedResultOffer,
        native_context: &Json,
    ) -> io::Result<RemoteReceiptIntent> {
        self.verify()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.root)
            .map_err(|_| invalid())?;
        let intent = RemoteReceiptIntent::new(&offer.encode(), native_context)?;
        let previous = self.read_intents()?;
        if let Some(old) = previous.iter().find(|old| old.id == intent.id) {
            return if old == &intent {
                Ok(old.clone())
            } else {
                Err(invalid())
            };
        }
        if previous.len() >= MAX_INTENTS
            || previous
                .iter()
                .any(|old| old.allocation() == intent.allocation())
        {
            return Err(invalid());
        }
        self.root.filesystem().write_new_file(
            Path::new(&intent.name()),
            intent.record(self)?.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.root.sync()?;
        self.verify()?;
        let saved = self.read_intents()?;
        if !saved.contains(&intent) {
            return Err(invalid());
        }
        Ok(intent)
    }
    /// Read at most 64 native selections, including interrupted or completed attempts. Completion
    /// must be checked independently in the fleet's retained local-review ledger. Corrupt/partial
    /// records refuse without deletion, repair, new allocation or transport.
    pub fn receipt_intents(&self) -> io::Result<Vec<RemoteReceiptIntent>> {
        self.verify()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.root)
            .map_err(|_| invalid())?;
        self.read_intents()
    }
    fn read_intents(&self) -> io::Result<Vec<RemoteReceiptIntent>> {
        self.verify()?;
        let names = self
            .root
            .filesystem()
            .read_directory_names_bounded(Path::new(""), MAX_INTENTS + 3)?;
        let mut result = Vec::new();
        for name in names {
            let text = name.to_str().ok_or_else(invalid)?;
            if matches!(text, "store" | "allocations" | RECEIPT) {
                continue;
            }
            let id = text
                .strip_prefix("receipt-")
                .and_then(|s| s.strip_suffix(".json"))
                .ok_or_else(invalid)?;
            if id.len() != 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(invalid());
            }
            let file = self
                .root
                .filesystem()
                .read_only()
                .read_file(Path::new(&name))?;
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o077 != 0
                || metadata.len() > MAX_BYTES
            {
                return Err(invalid());
            }
            let mut bytes = Vec::new();
            file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err(invalid());
            }
            let raw = String::from_utf8(bytes).map_err(|_| invalid())?;
            let value = Json::parse(&raw).map_err(|_| invalid())?;
            let intent = RemoteReceiptIntent::new(
                value
                    .get("offer")
                    .and_then(Json::as_text)
                    .ok_or_else(invalid)?,
                value.get("context").ok_or_else(invalid)?,
            )?;
            if intent.id != id || intent.record(self)? != raw {
                return Err(invalid());
            }
            result.push(intent);
        }
        result.sort_by(|a, b| a.id.cmp(&b.id));
        self.verify()?;
        Ok(result)
    }
}
