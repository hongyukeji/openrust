mod authenticated_session_controller;
mod confirmable_password_controller;
mod confirmed_password_status_controller;
pub use authenticated_session_controller::AuthenticatedSessionController;
pub use confirmable_password_controller::ConfirmablePasswordController;
pub use confirmed_password_status_controller::ConfirmedPasswordStatusController;

use crate::{
    contracts,
    resolver::{Resolver, internal},
};
use openrust::{
    http::{HttpError, Request, Response},
    support::facades::Facade as _,
};
pub(crate) async fn respond<T: contracts::Response + ?Sized + 'static>(
    request: &mut Request,
) -> Result<Response, HttpError> {
    Resolver::<T>::getFacadeRoot()
        .map_err(internal)?
        .toResponse(request)
        .await
}
mod two_factor_authenticated_session_controller;
pub use two_factor_authenticated_session_controller::TwoFactorAuthenticatedSessionController;
mod two_factor_management;
pub use two_factor_management::{
    ConfirmedTwoFactorAuthenticationController, RecoveryCodeController,
    TwoFactorAuthenticationController, TwoFactorQrCodeController, TwoFactorSecretKeyController,
};
mod registered_user_controller;
pub use registered_user_controller::RegisteredUserController;
mod user_updates;
pub use user_updates::{PasswordController, ProfileInformationController};

mod email_verification;
pub use email_verification::{
    EmailVerificationNotificationController, EmailVerificationPromptController,
    VerifyEmailController,
};
