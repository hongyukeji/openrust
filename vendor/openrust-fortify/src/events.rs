use openrust::{
    auth::AuthIdentifier,
    contracts::{auth::Authenticatable, events::Event},
    events::EventContext,
};
macro_rules! account_events {
 ($($name:ident),+ $(,)?)=>{$(
    #[derive(Debug)]
    pub struct $name {pub user:AuthIdentifier}
    impl Event for $name {fn name(&self)->&str {concat!("OpenRust\\Fortify\\Events\\",stringify!($name))}}
    impl $name {
        pub fn user<'a,T:Authenticatable>(&self,context:&EventContext<'a>)->Option<&'a T> {context.arguments_for(self)?.first()?.downcast_ref()}
    }
 )+};
}
account_events!(
    TwoFactorAuthenticationEnabled,
    TwoFactorAuthenticationConfirmed,
    TwoFactorAuthenticationDisabled,
    RecoveryCodesGenerated,
    TwoFactorAuthenticationChallenged,
    TwoFactorAuthenticationFailed,
    ValidTwoFactorAuthenticationCodeProvided
);
pub struct RecoveryCodeReplaced {
    pub user: AuthIdentifier,
    pub code: zeroize::Zeroizing<String>,
}
impl Event for RecoveryCodeReplaced {
    fn name(&self) -> &str {
        "OpenRust\\Fortify\\Events\\RecoveryCodeReplaced"
    }
}
impl RecoveryCodeReplaced {
    pub fn user<'a, T: Authenticatable>(&self, context: &EventContext<'a>) -> Option<&'a T> {
        context.arguments_for(self)?.first()?.downcast_ref()
    }
}
impl std::fmt::Debug for RecoveryCodeReplaced {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryCodeReplaced")
            .field("user", &self.user)
            .field("code", &"[REDACTED]")
            .finish()
    }
}

/// Snapshot of the updated account after reset-token deletion has completed.
pub struct PasswordUpdatedViaController {
    pub user: std::sync::Arc<dyn Authenticatable>,
}
impl Event for PasswordUpdatedViaController {
    fn name(&self) -> &str {
        "OpenRust\\Fortify\\Events\\PasswordUpdatedViaController"
    }
}
