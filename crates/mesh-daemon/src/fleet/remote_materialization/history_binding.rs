//! Private native location binding. Never returned to a renderer or accepted from a peer.
use super::*;
use crate::ipc::Json;

impl RemoteInputDestination {
    pub(in crate::fleet) fn history_binding(&self) -> io::Result<Json> {
        self.verify()?;
        if self.protected.len() > 64 {
            return Err(invalid());
        }
        let value = Json::object([
            ("schema", Json::text("mesh.remote-review-location/v1")),
            (
                "store_path",
                Json::text(self.store_path.to_str().ok_or_else(invalid)?),
            ),
            ("store", Json::text(token(&self.store)?.directory_token())),
            (
                "parent_path",
                Json::text(self.parent_path.to_str().ok_or_else(invalid)?),
            ),
            ("parent", Json::text(token(&self.parent)?.directory_token())),
            (
                "protected",
                Json::Array(
                    self.protected
                        .iter()
                        .map(|p| Json::text(p.directory_token()))
                        .collect(),
                ),
            ),
        ]);
        if value.encode().len() > 16_384 {
            return Err(invalid());
        }
        Ok(value)
    }
    pub(in crate::fleet) fn from_history_binding(value: &Json) -> io::Result<Self> {
        if value.encode().len() > 16_384 {
            return Err(invalid());
        }
        let text = |key| value.get(key).and_then(Json::as_text).ok_or_else(invalid);
        let protected = value
            .get("protected")
            .and_then(Json::as_array)
            .ok_or_else(invalid)?;
        if protected.len() > 64 {
            return Err(invalid());
        }
        let protected = protected
            .iter()
            .map(|p| ProtectedWorkspaceRoot::from_directory_token(p.as_text().ok_or_else(invalid)?))
            .collect::<io::Result<Vec<_>>>()?;
        let destination = Self::admit(
            Path::new(text("store_path")?),
            ProtectedWorkspaceRoot::from_directory_token(text("store")?)?,
            Path::new(text("parent_path")?),
            ProtectedWorkspaceRoot::from_directory_token(text("parent")?)?,
            &protected,
        )?;
        if destination.history_binding()? != *value {
            return Err(invalid());
        }
        Ok(destination)
    }
}
