//! Passport's default Eloquent client and overridable model behavior.
use crate::{Passport, PhpArray, Scope};
use openrust::{
    contracts::{auth::Authenticatable, container::Container as _},
    database::{
        DatabaseError, DatabaseResult, Value,
        eloquent::{Attributes, Model, ModelMetadata, ModelState, UniqueId, casts::AttributeCast},
    },
};
use zeroize::Zeroizing;

/// Eloquent behavior required by the Passport client repository.
#[allow(non_snake_case)]
pub trait ClientModel: Model {
    fn newQuery(
        connection: &openrust::database::Connection,
    ) -> openrust::database::eloquent::Builder<'static, Self> {
        Self::queryOn(connection)
    }
    fn firstParty(&self) -> DatabaseResult<bool> {
        let key = if self.state().getAttributes().contains_key("user_id") {
            "user_id"
        } else {
            "owner_id"
        };
        Ok(!truthy(self.getAttributeValue(key)?.as_ref()))
    }
    fn confidential(&self) -> bool {
        truthy(self.state().getAttribute("secret"))
    }
    fn hasGrantType(&self, grantType: &str) -> DatabaseResult<bool> {
        Ok(self.grantTypes()?.contains_loose(grantType))
    }
    fn hasScope(&self, scope: &str) -> DatabaseResult<bool> {
        if matches!(
            self.state().getAttribute("scopes"),
            None | Some(Value::Null)
        ) {
            return Ok(true);
        }
        let scopes = array(self.getAttributeValue("scopes")?)?;
        scope_exists_in(scope, &scopes)
    }
    fn tokens(&self) -> openrust::database::eloquent::relations::HasMany<Self, crate::Token> {
        self.tokensFor::<crate::Token>()
    }
    fn tokensFor<T: crate::TokenModel>(
        &self,
    ) -> openrust::database::eloquent::relations::HasMany<Self, T> {
        openrust::database::eloquent::relations::HasMany::new(self.getKeyName(), "client_id")
    }
    fn authCodes(&self) -> openrust::database::eloquent::relations::HasMany<Self, crate::AuthCode> {
        openrust::database::eloquent::relations::HasMany::new(self.getKeyName(), "client_id")
    }
    fn owner(&self) -> openrust::database::eloquent::relations::MorphTo<Self> {
        openrust::database::eloquent::relations::MorphTo::new("owner_type", "owner_id")
    }
    fn user<U: Model>(&self) -> openrust::database::eloquent::relations::BelongsTo<Self, U> {
        openrust::database::eloquent::relations::BelongsTo::new(
            "user_id",
            U::from_state(ModelState::default()).getKeyName(),
        )
    }
    fn redirectUris(&self) -> DatabaseResult<PhpArray> {
        array(self.getAttributeValue("redirect_uris")?)
    }
    fn grantTypes(&self) -> DatabaseResult<PhpArray> {
        array(self.getAttributeValue("grant_types")?)
    }
    fn skipsAuthorization(&self, _user: &dyn Authenticatable, _scopes: &[Scope]) -> bool {
        false
    }
}

/// A client retains plaintext only on the instance that assigned its secret.
/// Its single ModelState is shared by all ordinary Eloquent lifecycle operations.
#[allow(non_snake_case)]
pub struct Client {
    attributes: ClientAttributes,
    uses_unique_ids: bool,
    pub plainSecret: Option<Zeroizing<String>>,
}
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("id", &self.getKey())
            .finish_non_exhaustive()
    }
}
impl Default for Client {
    fn default() -> Self {
        Self::from_state(ModelState::default())
    }
}
impl ClientModel for Client {}

// This private view of the same state delegates assignment to Eloquent's existing
// casts, including its application-local asynchronous hash service. It never saves
// or runs events/queries as another model type and does not duplicate attributes.
struct ClientAttributes(ModelState);
impl Model for ClientAttributes {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("oauth_clients")
            .uniqueId(UniqueId::Uuid)
            .guarded(&[])
            .hidden(&["secret"]);
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
            ("secret", AttributeCast::Hashed),
            ("grant_types", AttributeCast::Json),
            ("scopes", AttributeCast::Json),
            ("redirect_uris", AttributeCast::Json),
            ("personal_access_client", AttributeCast::Boolean),
            ("password_client", AttributeCast::Boolean),
            ("revoked", AttributeCast::Boolean),
        ]
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
}

#[allow(non_snake_case)]
impl Model for Client {
    fn uniqueIds() -> &'static [&'static str] {
        if client_uuids() { &["id"] } else { &[] }
    }
    fn uniqueIdsForInstance(&self) -> &'static [&'static str] {
        if self.uses_unique_ids { &["id"] } else { &[] }
    }
    fn getIncrementing() -> bool {
        !client_uuids()
    }
    fn usesUniqueIdsForInstance(&self) -> bool {
        self.uses_unique_ids
    }
    fn getIncrementingForInstance(&self) -> bool {
        !self.uses_unique_ids
    }
    fn metadata() -> &'static ModelMetadata {
        ClientAttributes::metadata()
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        ClientAttributes::casts()
    }
    fn from_state(state: ModelState) -> Self {
        Self {
            attributes: ClientAttributes(state),
            uses_unique_ids: client_uuids(),
            plainSecret: None,
        }
    }
    fn state(&self) -> &ModelState {
        self.attributes.state()
    }
    fn state_mut(&mut self) -> &mut ModelState {
        self.attributes.state_mut()
    }
    fn resolveConnectionName(&self) -> DatabaseResult<Option<std::borrow::Cow<'_, str>>> {
        if let Some(name) = self.getConnectionName() {
            return Ok(Some(std::borrow::Cow::Borrowed(name)));
        }
        let configured = openrust::support::facades::Config::get("passport.connection")
            .map_err(DatabaseError::Unavailable)?;
        configured_connection(configured).map(|name| name.map(std::borrow::Cow::Owned))
    }
    fn mutateAttributeForArray(
        &self,
        column: &str,
        value: Option<&Value>,
    ) -> DatabaseResult<Value> {
        if matches!(column, "plain_secret" | "plainSecret") {
            return Ok(self.getAttributeValue(column)?.unwrap_or(Value::Null));
        }
        self.attributes.mutateAttributeForArray(column, value)
    }
    fn getAttributeValue(&self, column: &str) -> DatabaseResult<Option<Value>> {
        match column {
            "plain_secret" | "plainSecret" => Ok(Some(
                self.plainSecret
                    .as_ref()
                    .map_or(Value::Null, |value| Value::Text(value.to_string())),
            )),
            "redirect_uris" => self.redirect_value().map(Some),
            "grant_types" => self.grant_value().map(Some),
            _ => self.attributes.getAttributeValue(column),
        }
    }
    fn fill(&mut self, attributes: Attributes) -> DatabaseResult<&mut Self> {
        let plain = incoming_secret(&attributes)?;
        let result = self.attributes.fill(attributes).map(|_| ());
        self.remember_secret(plain, &result);
        result?;
        Ok(self)
    }
    async fn fillAsync(&mut self, attributes: Attributes) -> DatabaseResult<&mut Self> {
        let plain = incoming_secret(&attributes)?;
        let result = self.attributes.fillAsync(attributes).await.map(|_| ());
        self.remember_secret(plain, &result);
        result?;
        Ok(self)
    }
    fn setAttributeValue(
        &mut self,
        column: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<&mut Self> {
        let value = value.into();
        let plain = if column == "secret" {
            Some(secret_input(&value)?)
        } else {
            None
        };
        let result = self.attributes.setAttributeValue(column, value).map(|_| ());
        self.remember_secret(plain, &result);
        result?;
        Ok(self)
    }
    fn setAttributeValueAsync<'a>(
        &'a mut self,
        column: &'a str,
        value: impl Into<Value>,
    ) -> impl Future<Output = DatabaseResult<&'a mut Self>> + Send + 'a {
        let value = value.into();
        async move {
            let plain = if column == "secret" {
                Some(secret_input(&value)?)
            } else {
                None
            };
            let result = self
                .attributes
                .setAttributeValueAsync(column, value)
                .await
                .map(|_| ());
            self.remember_secret(plain, &result);
            result?;
            Ok(self)
        }
    }
}
pub(crate) fn configured_connection(
    value: Option<serde_json::Value>,
) -> DatabaseResult<Option<String>> {
    let name = match value {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(name)) => Some(name),
        Some(serde_json::Value::Bool(value)) => Some(if value { "1" } else { "" }.into()),
        Some(serde_json::Value::Number(value)) => Some(value.to_string()),
        _ => return Err(DatabaseError::InvalidConfiguration("passport.connection")),
    };
    Ok(name.filter(|name| !name.is_empty() && name != "0"))
}
#[allow(non_snake_case)]
impl Client {
    /// Builds the package factory using this application’s Passport connection.
    pub fn factory() -> DatabaseResult<crate::database::factories::ClientFactory> {
        crate::database::factories::ClientFactory::new()
    }
    fn remember_secret(
        &mut self,
        plain: Option<Option<Zeroizing<String>>>,
        result: &DatabaseResult<()>,
    ) {
        if let Some(plain) = plain
            && (result.is_ok()
                || matches!(
                    result,
                    Err(DatabaseError::AttributeHashing(_))
                        | Err(DatabaseError::InvalidQuery(
                            "hashed attribute requires async assignment"
                        ))
                ))
        {
            self.plainSecret = plain;
        }
    }
    fn redirect_value(&self) -> DatabaseResult<Value> {
        let raw = self.state().getAttribute("redirect_uris");
        if raw.is_some_and(|raw| {
            truthy(Some(raw))
                || matches!(raw, Value::Json(value) if value.is_array() || value.is_object())
        }) {
            return array(self.attributes.getAttributeValue("redirect_uris")?)
                .map(|value| Value::Json(value.into()));
        }
        let legacy = self.state().getAttribute("redirect");
        if truthy(legacy) {
            let value = secret_input(legacy.ok_or_else(invalid)?)?.ok_or_else(invalid)?;
            Ok(Value::Json(serde_json::Value::Array(
                value.split(',').map(|v| v.into()).collect(),
            )))
        } else {
            Ok(Value::Json(serde_json::json!([])))
        }
    }
    fn grant_value(&self) -> DatabaseResult<Value> {
        if !matches!(
            self.state().getAttribute("grant_types"),
            None | Some(Value::Null)
        ) {
            return array(self.attributes.getAttributeValue("grant_types")?)
                .map(|value| Value::Json(value.into()));
        }
        let redirects = !self.redirectUris()?.is_empty();
        let confidential = self.confidential();
        let grants = [
            ("authorization_code", redirects),
            ("client_credentials", confidential && self.firstParty()?),
            ("implicit", redirects),
            (
                "password",
                truthy(self.getAttributeValue("password_client")?.as_ref()),
            ),
            (
                "personal_access",
                truthy(self.getAttributeValue("personal_access_client")?.as_ref()) && confidential,
            ),
            ("refresh_token", true),
            ("urn:ietf:params:oauth:grant-type:device_code", true),
        ]
        .into_iter()
        .filter_map(|(name, enabled)| enabled.then_some(name))
        .collect::<Vec<_>>();
        Ok(Value::Json(serde_json::json!(grants)))
    }
}
fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("Passport client attribute")
}
fn array(value: Option<Value>) -> DatabaseResult<PhpArray> {
    match value {
        Some(Value::Json(value)) => PhpArray::try_from(value),
        _ => Err(invalid()),
    }
}
fn incoming_secret(attributes: &Attributes) -> DatabaseResult<Option<Option<Zeroizing<String>>>> {
    attributes.get("secret").map(secret_input).transpose()
}
fn secret_input(value: &Value) -> DatabaseResult<Option<Zeroizing<String>>> {
    let text = match value {
        Value::Null => return Ok(None),
        Value::Text(text) => text.to_owned(),
        Value::Bool(value) => if *value { "1" } else { "" }.to_owned(),
        Value::Integer(value) => value.to_string(),
        Value::Unsigned(value) => value.to_string(),
        Value::Real(value) if value.is_finite() => value.to_string(),
        _ => return Err(invalid()),
    };
    Ok(Some(Zeroizing::new(text)))
}
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Text(value)) => !value.is_empty() && value != "0",
        Some(Value::Bytes(value)) => !value.is_empty() && value != b"0",
        Some(Value::Integer(value)) => *value != 0,
        Some(Value::Unsigned(value)) => *value != 0,
        Some(Value::Real(value)) => *value != 0.0,
        Some(Value::Json(value)) => match value {
            serde_json::Value::Null | serde_json::Value::Bool(false) => false,
            serde_json::Value::Bool(true) => true,
            serde_json::Value::String(value) => !value.is_empty() && value != "0",
            serde_json::Value::Number(value) => value.as_f64() != Some(0.0),
            serde_json::Value::Array(value) => !value.is_empty(),
            serde_json::Value::Object(value) => !value.is_empty(),
        },
        Some(Value::Bool(true)) => true,
        _ => true,
    }
}

pub(crate) fn scope_exists_in(scope: &str, scopes: &PhpArray) -> DatabaseResult<bool> {
    let passport = openrust::container::Container::getInstance()
        .make::<Passport>()
        .map_err(DatabaseError::Unavailable)?;
    if !passport
        .withInheritedScopes
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Ok(scopes.contains_loose(scope));
    }
    Ok(scope
        .match_indices(':')
        .map(|(index, _)| &scope[..index])
        .chain(std::iter::once(scope))
        .any(|candidate| scopes.contains_loose(candidate)))
}

fn client_uuids() -> bool {
    openrust::container::Container::getInstance()
        .make::<Passport>()
        .map(|passport| {
            passport
                .clientUuids
                .load(std::sync::atomic::Ordering::Relaxed)
        })
        .unwrap_or(true)
}
