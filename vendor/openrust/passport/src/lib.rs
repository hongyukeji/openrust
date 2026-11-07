//! OpenRust Passport 的协议、客户端模型与持久读取子范围。
//!
//! 固定对应 Laravel Passport v13.10.0 与 League oauth2-server 9.4.1。
//! 当前包含客户端模型/读取、PKCE、scope、授权码与刷新编排及原子协议存储。
//! 不会自动注册授权端点；Client模型保存、认证权威适配及真实组合验收尚未闭合。
//! 不能将协议候选视为完整 AS/OP/SSO 已交付。
//!
//! ```
//! use openrust_passport::{BridgeClient, Scope, ScopeRepository};
//!
//! let entity = BridgeClient::new(
//!     "client", Some("应用".into()), vec![], false, None,
//!     vec!["authorization_code".into()],
//! );
//! assert!(entity.supports_grant_type("authorization_code"));
//! let scopes = ScopeRepository::new([Scope { id: "read".into(), description: "读取".into() }]);
//! assert!(scopes.has_scope("read"));
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod access_signer;
mod authorization_response;
mod authorization_server;
mod bridge;
mod bridge_repository;
mod client;
mod database_grants;
mod error;
mod grant_clock;
mod grant_config;
mod grant_records;
mod grant_storage;
mod grant_storage_error;
mod identifier_retry;
mod migrations;
mod parameters;
mod php_array;
mod pkce;
mod protocol_token;
mod provider;
mod redirect_uri;
mod repository;
mod scopes;
mod token_controller;
mod token_response;

pub use access_signer::{
    AccessSigningError, OAuthAccessSigner, OAuthAccessVerifier, PassportAccessClaims,
    validate_key_pair,
};
pub use authorization_response::{AuthorizationFailure, AuthorizationRedirect};
pub use authorization_server::{AuthorizationRequest, AuthorizationServer};
pub use database_grants::DatabaseGrantRepository;
pub use grant_clock::{GrantClock, SystemGrantClock};
pub use grant_config::{GrantConfig, GrantLifetime};
pub use grant_records::{
    AccessRecord, ClientSnapshot, CodeRecord, DatabaseGrantAuthority, GrantAuthority,
    GrantAuthorityBinding, GrantOperation, GrantUserKey, GrantValidation, RefreshRecord,
    RefreshRedemption, TokenRecords,
};
pub use protocol_token::generate_client_secret;
pub use redirect_uri::{
    RedirectUriError, ValidatedRedirectUri, matches_registered_redirect_uri,
    validate_token_redirect_uri,
};
pub use token_controller::AccessTokenController;
pub use token_response::TokenResponse;

pub use bridge::BridgeClient;
pub use bridge_repository::BridgeClientRepository;
pub use client::{Client, ClientFactory, ClientModel, ClientModelFactory};
pub use error::OAuthError;
pub use parameters::OAuthParameters;
pub use php_array::PhpArray;
pub use pkce::{CodeChallenge, CodeChallengeMethod, PkceError, verify_optional_challenge};
pub use repository::{
    ClientLookup, ClientLookupContext, ClientRecord, ClientRepository,
    register_client_repository_scope,
};
pub use scopes::{Scope, ScopeRepository};

#[cfg(test)]
mod public_api_tests;

#[cfg(test)]
mod test_keys;

pub use grant_storage::GrantStorage;

pub use grant_storage_error::{GrantIdentifierKind, GrantStorageError, GrantStorageResult};

pub use migrations::{
    CreateOAuthAccessTokensTable, CreateOAuthAuthCodesTable, CreateOAuthClientsTable,
    CreateOAuthRefreshTokensTable, default_passport_migrations,
};
pub use provider::{PassportConfig, PassportServiceProvider, validate_pair};
