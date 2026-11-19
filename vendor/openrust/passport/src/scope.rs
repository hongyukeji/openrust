//! A scope identifier and description exposed by Passport.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Scope {
    pub id: String,
    pub description: String,
}
#[allow(non_snake_case)]
impl Scope {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
        }
    }
    pub fn toArray(&self) -> serde_json::Value {
        serde_json::json!({"id":self.id,"description":self.description})
    }
    pub fn toJson(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}
