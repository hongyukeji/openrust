use serde_json::Value;
use zeroize::Zeroizing;
/// Secrets are explicit and are neither serializable nor included in Debug.
#[derive(Clone)]
pub struct Token {
    pub token: Zeroizing<String>,
    pub tokenSecret: Option<Zeroizing<String>>,
    pub refreshToken: Option<Zeroizing<String>>,
    pub expiresIn: Option<u64>,
    pub approvedScopes: Vec<String>,
}
#[derive(Clone)]
pub struct User {
    pub id: Value,
    pub nickname: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
    pub avatar: Option<String>,
    pub user: Value,
    pub attributes: serde_json::Map<String, Value>,
    pub credentials: Option<Token>,
}
impl User {
    pub fn get_id(&self) -> &Value {
        &self.id
    }
    pub fn getNickname(&self) -> Option<&str> {
        self.nickname.as_deref()
    }
    pub fn get_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    pub fn getEmail(&self) -> Option<&str> {
        self.email.as_deref()
    }
    pub fn getAvatar(&self) -> Option<&str> {
        self.avatar.as_deref()
    }
    pub fn getRaw(&self) -> &Value {
        &self.user
    }
    pub fn setRaw(mut self, user: Value) -> Self {
        self.user = user;
        self
    }
    pub fn map(mut self, attributes: serde_json::Map<String, Value>) -> Self {
        self.id = attributes.get("id").cloned().unwrap_or(Value::Null);
        for (field, target) in [
            ("nickname", &mut self.nickname),
            ("name", &mut self.name),
            ("email", &mut self.email),
            ("avatar", &mut self.avatar),
        ] {
            *target = attributes
                .get(field)
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        self.attributes = attributes;
        self
    }
    pub fn from_attributes(raw: Value, attributes: serde_json::Map<String, Value>) -> Self {
        Self {
            id: Value::Null,
            nickname: None,
            name: None,
            email: None,
            avatar: None,
            user: raw,
            attributes: Default::default(),
            credentials: None,
        }
        .map(attributes)
    }
}
