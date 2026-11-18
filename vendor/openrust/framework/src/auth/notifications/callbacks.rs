use std::sync::{Arc, RwLock};

use crate::{
    auth::{AuthError, AuthResult},
    contracts::{
        auth::{CanResetPassword, MustVerifyEmail},
        container::Container as _,
    },
    notifications::messages::MailDelivery,
    support::facades::current_container,
};

type ResetUrl = dyn Fn(&dyn CanResetPassword, &str) -> AuthResult<String> + Send + Sync;
type ResetMail = dyn Fn(&dyn CanResetPassword, &str) -> AuthResult<MailDelivery> + Send + Sync;
type VerifyUrl = dyn Fn(&dyn MustVerifyEmail) -> AuthResult<String> + Send + Sync;
type VerifyMail = dyn Fn(&dyn MustVerifyEmail, &str) -> AuthResult<MailDelivery> + Send + Sync;

#[derive(Default)]
pub(crate) struct MailCallbacks {
    pub(super) reset_url: Option<Arc<ResetUrl>>,
    pub(super) reset_mail: Option<Arc<ResetMail>>,
    pub(super) verify_url: Option<Arc<VerifyUrl>>,
    pub(super) verify_mail: Option<Arc<VerifyMail>>,
}

impl MailCallbacks {
    pub(super) fn get() -> AuthResult<Arc<RwLock<Self>>> {
        current_container()
            .ok_or(AuthError::Configuration("application scope"))?
            .make::<RwLock<Self>>()
            .map_err(AuthError::backend)
    }
}
