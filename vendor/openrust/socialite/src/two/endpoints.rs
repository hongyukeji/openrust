/// Explicit endpoint overrides for self-hosted or controlled protocol services.
#[derive(Clone)]
pub struct ProviderEndpoints {
    pub authorize: String,
    pub token: String,
    pub user: String,
    pub email: Option<String>,
}
impl ProviderEndpoints {
    pub fn new(
        authorize: impl Into<String>,
        token: impl Into<String>,
        user: impl Into<String>,
    ) -> Self {
        Self {
            authorize: authorize.into(),
            token: token.into(),
            user: user.into(),
            email: None,
        }
    }
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }
}
pub(super) fn value(raw: &serde_json::Value, path: &[&str]) -> serde_json::Value {
    path.iter()
        .try_fold(raw, |raw, k| match raw {
            serde_json::Value::Array(a) => k.parse::<usize>().ok().and_then(|i| a.get(i)),
            _ => raw.get(*k),
        })
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}
pub(super) fn mapped(
    raw: serde_json::Value,
    fields: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) -> super::User {
    super::User::from_attributes(
        raw,
        fields.into_iter().map(|(k, v)| (k.into(), v)).collect(),
    )
}
