//! 固定Fortify的双因素provider、模型能力与认证动作。
//!
//! 复用框架Guard、Provider、Session和加密器；包回退与安装默认分别公开。

pub(crate) mod actions;
mod api;
mod api_audit;
mod api_http;
mod api_management;
mod api_provider;
mod authority;
mod authority_migration;
mod authorization;
mod challenge_migration;
mod config;
mod credential_migration;
mod credentials;
mod database;
mod engine;
mod http;
pub(crate) mod login;
mod migration;
mod options;
mod provider;
mod qr;
mod responses;
mod runtime;
mod state;
mod throttle;
mod verification;

pub use engine::{GoogleTwoFactorAuthenticationProvider, TwoFactorAuthenticationProvider};
pub use options::TwoFactorOptions;
pub use state::{TwoFactorAuthenticatable, TwoFactorState};
pub use verification::TwoFactorCodeVerification;

pub use actions::{RecoveryCode, TwoFactorActions, generate_recovery_code};
pub use api::{FortifyJwtApi, FortifyJwtApiConfig};
pub use api_audit::{ApiFactorManagementAuditSink, DatabaseApiLoginAudit};
pub(crate) use api_management::ApiManagementAuthorization;
pub use api_management::{JwtApiFactorManagement, JwtApiFactorManagementOptions};
pub use api_provider::FortifyJwtApiServiceProvider;
pub use authority::{
    DatabaseFactorAuthority, FactorAuthorityMutation, FactorAuthoritySnapshot,
    FactorAuthorityStatus,
};
pub use authority_migration::FactorAuthorityMigration;
pub use authorization::authorize_session_factor;
pub use challenge_migration::CredentialChallengeMigration;
pub use credential_migration::CredentialFactorMigration;
pub use credentials::CredentialAuthority;
#[cfg(feature = "auth-oidc")]
pub(crate) use credentials::PreparedSessionAttachment;
pub(crate) use credentials::{
    ChallengeCommitCheck, CredentialChallengeVerification, CredentialLoginDecision,
    PreparedCredentialChallenge,
};
pub use database::DatabaseTwoFactorState;

pub use qr::two_factor_qr_code_svg;

pub use login::{AuthenticateUsing, FortifyLogin, FortifyLoginOutcome};

pub use provider::FortifyServiceProvider;

pub use config::{FortifyConfig, FortifyLimiters};

pub use migration::FortifyTwoFactorMigration;
pub use responses::*;

pub use runtime::{ConfirmPasswordsUsing, Fortify, FortifyCallbacks, RequireFactorAuthority};

pub use throttle::{FortifyLimit, FortifyRateLimiters, Lockout};

#[cfg(feature = "auth-fortify")]
pub(crate) fn configured_confirmation(
    config: &crate::config::repository::ConfigRepository,
) -> crate::Result<bool> {
    options::confirmation_required(config)
}

#[cfg(test)]
#[path = "../../tests/unit/auth/fortify_helpers.rs"]
mod test_helpers;
#[cfg(test)]
pub(crate) use test_helpers::test_field_encrypter;

#[cfg(test)]
#[path = "../../tests/unit/auth/fortify_event_names.rs"]
mod event_name_tests;
