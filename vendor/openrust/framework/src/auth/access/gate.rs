use crate::contracts::auth::Authenticatable;

/// Evaluates abilities for the user resolved in the current scope.
pub struct Gate<'scope> {
    _user: Option<&'scope dyn Authenticatable>,
}
