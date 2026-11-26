//! A scope identifier and description exposed by Passport.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Scope {
    pub id: String,
    pub description: String,
}
impl Scope {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
        }
    }
    pub fn to_array(&self) -> serde_json::Value {
        serde_json::json!({"id":self.id,"description":self.description})
    }
    pub fn to_json(&self) -> serde_json::Result<String> {
        self.to_json_with_options(crate::ScopeJsonOptions::default())
    }
    pub fn to_json_with_options(
        &self,
        options: crate::ScopeJsonOptions,
    ) -> serde_json::Result<String> {
        Ok(options.encode(self))
    }
}
