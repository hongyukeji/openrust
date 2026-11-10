/// Checks request origin or the current session's CSRF token.
pub struct PreventRequestForgery {
    _private: (),
}
