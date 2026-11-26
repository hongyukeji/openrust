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
pub trait ClientModel: Model {
    fn new_query(
        connection: &openrust::database::Connection,
    ) -> openrust::database::eloquent::Builder<'static, Self> {
        Self::query_on(connection)
    }
    fn first_party(&self) -> DatabaseResult<bool> {
        let key = if self.state().get_attributes().contains_key("user_id") {
            "user_id"
        } else {
            "owner_id"
        };
        Ok(!truthy(self.get_attribute_value(key)?.as_ref()))
    }
    fn confidential(&self) -> bool {
        truthy(self.state().get_attribute("secret"))
    }
    fn has_grant_type(&self, grant_type: &str) -> DatabaseResult<bool> {
        Ok(self.grant_types()?.contains_loose(grant_type))
    }
    fn has_scope(&self, scope: &str) -> DatabaseResult<bool> {
        if matches!(
            self.state().get_attribute("scopes"),
            None | Some(Value::Null)
        ) {
            return Ok(true);
        }
        let scopes = array(self.get_attribute_value("scopes")?)?;
        scope_exists_in(scope, &scopes)
    }
    fn tokens(&self) -> openrust::database::eloquent::relations::HasMany<Self, crate::Token> {
        self.tokens_for::<crate::Token>()
    }
    fn tokens_for<T: crate::TokenModel>(
        &self,
    ) -> openrust::database::eloquent::relations::HasMany<Self, T> {
        openrust::database::eloquent::relations::HasMany::new(self.get_key_name(), "client_id")
    }
    fn auth_codes(
        &self,
    ) -> openrust::database::eloquent::relations::HasMany<Self, crate::AuthCode> {
        self.auth_codes_for::<crate::AuthCode>()
    }
    fn auth_codes_for<A: Model>(
        &self,
    ) -> openrust::database::eloquent::relations::HasMany<Self, A> {
        openrust::database::eloquent::relations::HasMany::new(self.get_key_name(), "client_id")
    }
    fn owner(&self) -> openrust::database::eloquent::relations::MorphTo<Self> {
        openrust::database::eloquent::relations::MorphTo::new("owner_type", "owner_id")
    }
    fn user<U: Model>(&self) -> openrust::database::eloquent::relations::BelongsTo<Self, U> {
        openrust::database::eloquent::relations::BelongsTo::new(
            "user_id",
            U::from_state(ModelState::default()).get_key_name(),
        )
    }
    fn redirect_uris(&self) -> DatabaseResult<PhpArray> {
        array(self.get_attribute_value("redirect_uris")?)
    }
    fn grant_types(&self) -> DatabaseResult<PhpArray> {
        array(self.get_attribute_value("grant_types")?)
    }
    fn skips_authorization(&self, _user: &dyn Authenticatable, _scopes: &[Scope]) -> bool {
        false
    }
}

/// A client retains plaintext only on the instance that assigned its secret.
/// Its single ModelState is shared by all ordinary Eloquent lifecycle operations.
pub struct Client {
    attributes: ClientAttributes,
    uses_unique_ids: bool,
    pub plain_secret: Option<Zeroizing<String>>,
}
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("id", &self.get_key())
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
            .unique_id(UniqueId::Uuid)
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

impl Model for Client {
    fn unique_ids() -> &'static [&'static str] {
        if client_uuids() { &["id"] } else { &[] }
    }
    fn unique_ids_for_instance(&self) -> &'static [&'static str] {
        if self.uses_unique_ids { &["id"] } else { &[] }
    }
    fn get_incrementing() -> bool {
        !client_uuids()
    }
    fn uses_unique_ids_for_instance(&self) -> bool {
        self.uses_unique_ids
    }
    fn get_incrementing_for_instance(&self) -> bool {
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
            plain_secret: None,
        }
    }
    fn state(&self) -> &ModelState {
        self.attributes.state()
    }
    fn state_mut(&mut self) -> &mut ModelState {
        self.attributes.state_mut()
    }
    fn resolve_connection_name(&self) -> DatabaseResult<Option<std::borrow::Cow<'_, str>>> {
        if let Some(name) = self.get_connection_name() {
            return Ok(Some(std::borrow::Cow::Borrowed(name)));
        }
        let configured = openrust::support::facades::Config::get("passport.connection")
            .map_err(DatabaseError::Unavailable)?;
        configured_connection(configured).map(|name| name.map(std::borrow::Cow::Owned))
    }
    fn mutate_attribute_for_array(
        &self,
        column: &str,
        value: Option<&Value>,
    ) -> DatabaseResult<Value> {
        if matches!(column, "plain_secret" | "plainSecret") {
            return Ok(self.get_attribute_value(column)?.unwrap_or(Value::Null));
        }
        self.attributes.mutate_attribute_for_array(column, value)
    }
    fn get_attribute_value(&self, column: &str) -> DatabaseResult<Option<Value>> {
        match column {
            "plain_secret" | "plainSecret" => Ok(Some(
                self.plain_secret
                    .as_ref()
                    .map_or(Value::Null, |value| Value::Text(value.to_string())),
            )),
            "redirect_uris" => self.redirect_value().map(Some),
            "grant_types" => self.grant_value().map(Some),
            _ => self.attributes.get_attribute_value(column),
        }
    }
    fn fill(&mut self, attributes: Attributes) -> DatabaseResult<&mut Self> {
        for (name, value) in attributes {
            if name == "secret" {
                self.set_attribute_value(&name, value)?;
            } else {
                self.attributes.fill(Attributes::from([(name, value)]))?;
            }
        }
        Ok(self)
    }
    async fn fill_async(&mut self, attributes: Attributes) -> DatabaseResult<&mut Self> {
        for (name, value) in attributes {
            if name == "secret" {
                self.set_attribute_value_async(&name, value).await?;
            } else {
                self.attributes
                    .fill_async(Attributes::from([(name, value)]))
                    .await?;
            }
        }
        Ok(self)
    }
    fn set_attribute_value(
        &mut self,
        column: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<&mut Self> {
        let value = value.into();
        let value = if column == "secret" {
            self.assign_secret(&value)?
        } else {
            value
        };
        self.attributes.set_attribute_value(column, value)?;
        Ok(self)
    }
    fn set_attribute_value_async<'a>(
        &'a mut self,
        column: &'a str,
        value: impl Into<Value>,
    ) -> impl Future<Output = DatabaseResult<&'a mut Self>> + Send + 'a {
        let value = value.into();
        async move {
            let value = if column == "secret" {
                self.assign_secret(&value)?
            } else {
                value
            };
            self.attributes
                .set_attribute_value_async(column, value)
                .await?;
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
impl Client {
    /// Builds the package factory using this application’s Passport connection.
    pub fn factory() -> DatabaseResult<crate::database::factories::ClientFactory> {
        crate::database::factories::ClientFactory::new()
    }
    fn assign_secret(&mut self, input: &Value) -> DatabaseResult<Value> {
        let plain = secret_input(input)?;
        let value = plain
            .as_ref()
            .map_or(Value::Null, |plain| Value::Text(plain.to_string()));
        // The real PHP setter updates plainSecret before entering the hash boundary.
        self.plain_secret = plain;
        Ok(value)
    }
    fn redirect_value(&self) -> DatabaseResult<Value> {
        let raw = self.state().get_attribute("redirect_uris");
        if raw.is_some_and(|raw| {
            truthy(Some(raw))
                || matches!(raw, Value::Json(value) if value.is_array() || value.is_object())
        }) {
            return array(self.attributes.get_attribute_value("redirect_uris")?)
                .map(|value| Value::Json(value.into()));
        }
        let legacy = self.state().get_attribute("redirect");
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
            self.state().get_attribute("grant_types"),
            None | Some(Value::Null)
        ) {
            return array(self.attributes.get_attribute_value("grant_types")?)
                .map(|value| Value::Json(value.into()));
        }
        let redirects = !self.redirect_uris()?.is_empty();
        let confidential = self.confidential();
        let grants = [
            ("authorization_code", redirects),
            ("client_credentials", confidential && self.first_party()?),
            ("implicit", redirects),
            (
                "password",
                truthy(self.get_attribute_value("password_client")?.as_ref()),
            ),
            (
                "personal_access",
                truthy(self.get_attribute_value("personal_access_client")?.as_ref())
                    && confidential,
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
fn secret_input(value: &Value) -> DatabaseResult<Option<Zeroizing<String>>> {
    let text = match value {
        Value::Null => return Ok(None),
        Value::Text(text) => text.to_owned(),
        Value::Bool(value) => if *value { "1" } else { "" }.to_owned(),
        Value::Integer(value) => value.to_string(),
        Value::Unsigned(value) => value.to_string(),
        Value::Real(value) => openrust::support::php_float_string(*value),
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
    let passport = openrust::container::Container::get_instance()
        .make::<Passport>()
        .map_err(DatabaseError::Unavailable)?;
    if !passport
        .with_inherited_scopes
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
    openrust::container::Container::get_instance()
        .make::<Passport>()
        .map(|passport| {
            passport
                .client_uuids
                .load(std::sync::atomic::Ordering::Relaxed)
        })
        .unwrap_or(true)
}
