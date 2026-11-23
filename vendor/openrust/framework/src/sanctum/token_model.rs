use subtle::ConstantTimeEq as _;

use super::{
    personal_access_token::token_hash,
    token_store::{credential_key, text},
};
use crate::database::{
    Connection, DatabaseResult,
    eloquent::{Builder, Model, ModelMetadata, ModelState, casts::AttributeCast},
};

/// An Eloquent token model with an overridable Sanctum credential lookup.
pub trait PersonalAccessTokenModel: Model {
    fn find_token<'a>(
        query: Builder<'static, Self>,
        connection: &'a Connection,
        credential: &'a str,
    ) -> impl Future<Output = DatabaseResult<Option<Self>>> + Send + 'a {
        Self::find_token_by_digest(query, connection, credential)
    }

    /// Standard lookup for overrides that retain Sanctum's SHA-256 credential verification.
    fn find_token_by_digest<'a>(
        mut query: Builder<'static, Self>,
        connection: &'a Connection,
        credential: &'a str,
    ) -> impl Future<Output = DatabaseResult<Option<Self>>> + Send + 'a {
        async move {
            let digest = if let Some((id, secret)) = credential.split_once('|') {
                if id.is_empty() || secret.is_empty() {
                    return Ok(None);
                }
                let Some(key) = credential_key::<Self>(id, connection) else {
                    return Ok(None);
                };
                query.where_key(key);
                token_hash(secret)
            } else if credential.is_empty() {
                return Ok(None);
            } else {
                let digest = token_hash(credential);
                query.r#where("token", "=", digest.as_str());
                digest
            };
            let Some(model) = query.first().await? else {
                return Ok(None);
            };
            if !bool::from(text(&model, "token")?.as_bytes().ct_eq(digest.as_bytes())) {
                return Ok(None);
            }
            Ok(Some(model))
        }
    }
}

/// The Eloquent model used for typed queries against Sanctum's default table.
pub struct DefaultPersonalAccessToken(ModelState);

impl Model for DefaultPersonalAccessToken {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata =
            ModelMetadata::new("personal_access_tokens").hidden(&["token"]);
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
            ("abilities", AttributeCast::Json),
            ("last_used_at", AttributeCast::DateTime),
            ("expires_at", AttributeCast::DateTime),
        ]
    }
}

impl PersonalAccessTokenModel for DefaultPersonalAccessToken {}
