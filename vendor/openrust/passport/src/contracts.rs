/// Native scope contract. Database errors preserve application inheritance configuration failures.
pub trait ScopeAuthorizable {
    fn can(&self, scope: &str) -> openrust::database::DatabaseResult<bool>;
    fn cant(&self, scope: &str) -> openrust::database::DatabaseResult<bool> {
        self.can(scope).map(|value| !value)
    }
    fn transient(&self) -> bool;
}
impl ScopeAuthorizable for crate::AccessToken {
    fn can(&self, scope: &str) -> openrust::database::DatabaseResult<bool> {
        self.can(scope)
    }
    fn transient(&self) -> bool {
        self.transient()
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct TransientToken;
impl TransientToken {
    pub fn can(&self, _: &str) -> bool {
        true
    }
    pub fn cant(&self, _: &str) -> bool {
        false
    }
    pub fn transient(&self) -> bool {
        true
    }
}
impl ScopeAuthorizable for TransientToken {
    fn can(&self, _: &str) -> openrust::database::DatabaseResult<bool> {
        Ok(true)
    }
    fn transient(&self) -> bool {
        true
    }
}
