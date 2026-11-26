use super::{Token, User};
use crate::{Error, Result};
use openrust::{
    contracts::session::Session as _,
    http::{Request, client::Factory},
};
use serde_json::{Map, Value};
use std::{future::Future, pin::Pin, sync::Arc};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;
pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
#[derive(Clone)]
pub struct ProviderConfig {
    pub client_id: String,
    pub client_secret: Zeroizing<String>,
    pub redirect: String,
    pub scopes: Vec<String>,
}
impl ProviderConfig {
    pub fn from_config(value: &Value) -> Result<Self> {
        fn string(value: &Value, key: &'static str) -> Result<String> {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(Error::Configuration(key))
        }
        let scopes = match value.get("scopes") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(values)) => values
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or(Error::Configuration("scopes"))
                })
                .collect::<Result<_>>()?,
            _ => return Err(Error::Configuration("scopes")),
        };
        Ok(Self {
            client_id: string(value, "client_id")?,
            client_secret: Zeroizing::new(string(value, "client_secret")?),
            redirect: string(value, "redirect")?,
            scopes,
        })
    }
}
/// Per-request provider options. They are never stored in the application's driver definition.
#[derive(Default)]
pub struct ProviderOptions {
    pub fields: Option<Vec<String>>,
    pub graph_version: Option<String>,
    pub popup: bool,
    pub re_request: bool,
    pub expected_nonce: Option<String>,
    pub open_id: Option<String>,
    pub with_union_id: bool,
}
/// Driver protocol customization, shared across request-owned provider instances.
pub trait ProviderDefinition: Send + Sync {
    fn get_auth_url(&self) -> &str;
    fn get_token_url(&self) -> &str;
    fn scopes(&self) -> &[&str] {
        &[]
    }
    fn scope_separator(&self) -> &str {
        ","
    }
    fn uses_pkce(&self) -> bool {
        false
    }
    fn uses_rfc3986(&self) -> bool {
        false
    }
    fn refresh_token_includes_secret(&self) -> bool {
        true
    }
    fn token_basic_auth(&self) -> bool {
        false
    }
    fn direct_token_basic_auth(&self) -> bool {
        self.token_basic_auth()
    }
    fn customize_code_fields(
        &self,
        _fields: &mut Map<String, Value>,
        _scopes: &[String],
        _bot_user: bool,
        _stateless: bool,
    ) {
    }
    fn normalize_token_response(&self, response: Value, _bot_user: bool) -> Result<Value> {
        Ok(response)
    }
    /// Override non-standard token transports while retaining the shared state validation.
    fn get_access_token_response<'a>(
        &'a self,
        _http: &'a Factory,
        _config: &'a ProviderConfig,
        _fields: &'a Value,
    ) -> Option<ProviderFuture<'a, Value>> {
        None
    }
    fn refresh_token_response<'a>(
        &'a self,
        _http: &'a Factory,
        _config: &'a ProviderConfig,
        _refresh_token: &'a str,
    ) -> Option<ProviderFuture<'a, Value>> {
        None
    }
    /// Stores protocol context in the request-owned options, never in the shared driver.
    fn configure_token_response(
        &self,
        _response: &Value,
        _options: &mut ProviderOptions,
    ) -> Result<()> {
        Ok(())
    }
    fn get_user_by_token<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
    ) -> ProviderFuture<'a, Value>;
    fn get_user_by_token_with_config<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
        _config: &'a ProviderConfig,
    ) -> ProviderFuture<'a, Value> {
        self.get_user_by_token(http, token, scopes)
    }
    fn normalize_refresh_token_response(
        &self,
        response: Value,
        _refresh_token: &str,
    ) -> Result<Value> {
        Ok(response)
    }
    fn authorization_endpoint(&self, _options: &ProviderOptions) -> String {
        self.get_auth_url().into()
    }
    fn token_endpoint(&self, _options: &ProviderOptions) -> String {
        self.get_token_url().into()
    }
    fn token_accepts_json(&self) -> bool {
        true
    }
    fn customize_code_fields_with_options(
        &self,
        fields: &mut Map<String, Value>,
        scopes: &[String],
        bot_user: bool,
        stateless: bool,
        _options: &ProviderOptions,
    ) {
        self.customize_code_fields(fields, scopes, bot_user, stateless);
    }
    fn get_user_by_token_with_options<'a>(
        &'a self,
        http: &'a Factory,
        token: &'a str,
        scopes: &'a [String],
        config: &'a ProviderConfig,
        _options: &'a ProviderOptions,
        _parameters: &'a Map<String, Value>,
    ) -> ProviderFuture<'a, Value> {
        self.get_user_by_token_with_config(http, token, scopes, config)
    }
    fn map_user_to_object(&self, user: Value) -> Result<User>;
}
pub struct AbstractProvider<'a> {
    request: &'a mut Request,
    config: ProviderConfig,
    http: Arc<Factory>,
    definition: Arc<dyn ProviderDefinition>,
    parameters: Map<String, Value>,
    scopes: Vec<String>,
    stateless: bool,
    uses_pkce: bool,
    bot_user: bool,
    options: ProviderOptions,
    last_token: Option<Zeroizing<String>>,
    user: Option<User>,
}
impl<'a> AbstractProvider<'a> {
    pub fn new(
        request: &'a mut Request,
        config: ProviderConfig,
        http: Arc<Factory>,
        definition: Arc<dyn ProviderDefinition>,
    ) -> Self {
        let scopes = definition
            .scopes()
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let uses_pkce = definition.uses_pkce();
        let mut provider = Self {
            request,
            config,
            http,
            definition,
            parameters: Map::new(),
            scopes,
            stateless: false,
            uses_pkce,
            bot_user: false,
            options: ProviderOptions::default(),
            last_token: None,
            user: None,
        };
        provider.scopes(provider.config.scopes.clone());
        provider
    }
    pub fn scopes(&mut self, scopes: impl IntoIterator<Item = String>) -> &mut Self {
        for scope in scopes {
            if !self.scopes.contains(&scope) {
                self.scopes.push(scope);
            }
        }
        self
    }
    pub fn set_scopes(&mut self, scopes: Vec<String>) -> &mut Self {
        self.scopes.clear();
        self.scopes(scopes)
    }
    pub fn get_scopes(&self) -> &[String] {
        &self.scopes
    }
    pub fn with(&mut self, parameters: Map<String, Value>) -> &mut Self {
        self.parameters = parameters;
        self
    }
    pub fn stateless(&mut self) -> &mut Self {
        self.stateless = true;
        self
    }
    pub fn is_stateless(&self) -> bool {
        self.stateless
    }
    pub fn as_bot_user(&mut self) -> &mut Self {
        self.bot_user = true;
        self
    }
    pub fn fields(&mut self, fields: Vec<String>) -> &mut Self {
        self.options.fields = Some(fields);
        self
    }
    pub fn using_graph_version(&mut self, version: impl Into<String>) -> &mut Self {
        self.options.graph_version = Some(version.into());
        self
    }
    pub fn as_popup(&mut self) -> &mut Self {
        self.options.popup = true;
        self
    }
    pub fn re_request(&mut self) -> &mut Self {
        self.options.re_request = true;
        self
    }
    pub fn with_nonce(&mut self, nonce: impl Into<String>) -> &mut Self {
        self.options.expected_nonce = Some(nonce.into());
        self
    }
    pub fn set_open_id(&mut self, open_id: impl Into<String>) -> &mut Self {
        self.options.open_id = Some(open_id.into());
        self
    }
    pub fn with_union_id(&mut self, value: bool) -> &mut Self {
        self.options.with_union_id = value;
        self
    }
    pub fn last_token(&self) -> Option<&str> {
        self.last_token.as_ref().map(|token| token.as_str())
    }
    pub fn enable_pkce(&mut self) -> &mut Self {
        self.uses_pkce = true;
        self
    }
    pub fn redirect_url(&mut self, url: impl Into<String>) -> &mut Self {
        self.config.redirect = url.into();
        self
    }
    pub fn redirect(&mut self) -> Result<openrust::http::RedirectResponse> {
        let target = self.authorization_url()?;
        Ok(openrust::support::facades::Redirect::away(
            &target,
            Default::default(),
        )?)
    }
    pub fn authorization_url(&mut self) -> Result<String> {
        let mut fields = Map::from_iter([
            ("client_id".into(), self.config.client_id.clone().into()),
            ("redirect_uri".into(), self.config.redirect.clone().into()),
            (
                "scope".into(),
                self.scopes.join(self.definition.scope_separator()).into(),
            ),
            ("response_type".into(), "code".into()),
        ]);
        if !self.stateless {
            let state = random(40)?;
            self.request
                .session_mut()?
                .put("state", state.clone().into())?;
            fields.insert("state".into(), state.into());
        }
        if self.uses_pkce {
            let verifier = random(96)?;
            self.request
                .session_mut()?
                .put("code_verifier", verifier.clone().into())?;
            use base64::Engine as _;
            let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes());
            fields.insert(
                "code_challenge".into(),
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(digest.as_ref())
                    .into(),
            );
            fields.insert("code_challenge_method".into(), "S256".into());
        }
        fields.extend(self.parameters.clone());
        self.definition.customize_code_fields_with_options(
            &mut fields,
            &self.scopes,
            self.bot_user,
            self.stateless,
            &self.options,
        );
        let mut url = url::Url::parse(&self.definition.authorization_endpoint(&self.options))
            .map_err(|_| Error::Configuration("authorization URL"))?;
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in fields {
                pairs.append_pair(&key, &scalar(&value)?);
            }
        }
        if self.definition.uses_rfc3986() {
            let query = url.query().map(|q| {
                q.replace('+', "%20")
                    .replace('*', "%2A")
                    .replace("%7E", "~")
            });
            url.set_query(query.as_deref());
        }
        Ok(url.into())
    }
    pub async fn user(&mut self) -> Result<User> {
        if let Some(user) = &self.user {
            return Ok(user.clone());
        }
        if !self.stateless {
            let state = self.request.session_mut()?.pull("state");
            let incoming = self.request.input(Some("state")).await?;
            let valid = match (
                state.as_ref().and_then(Value::as_str),
                incoming.as_ref().and_then(Value::as_str),
            ) {
                (Some(a), Some(b)) => {
                    !a.is_empty() && a != "0" && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
                }
                _ => false,
            };
            if !valid {
                return Err(Error::InvalidState);
            }
        }
        let code = self
            .request
            .input(Some("code"))
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or(Error::MissingCode)?;
        let response = self.get_access_token_response(&code).await?;
        let credentials = self.credentials(&response)?;
        self.last_token = Some(credentials.token.clone());
        let raw = self
            .definition
            .get_user_by_token_with_options(
                &self.http,
                &credentials.token,
                &self.scopes,
                &self.config,
                &self.options,
                &self.parameters,
            )
            .await?;
        let mut user = self.definition.map_user_to_object(raw)?;
        user.credentials = Some(credentials);
        self.user = Some(user.clone());
        Ok(user)
    }
    pub async fn user_from_token(&mut self, token: &str) -> Result<User> {
        self.last_token = Some(Zeroizing::new(token.into()));
        let raw = self
            .definition
            .get_user_by_token_with_options(
                &self.http,
                token,
                &self.scopes,
                &self.config,
                &self.options,
                &self.parameters,
            )
            .await?;
        let mut user = self.definition.map_user_to_object(raw)?;
        user.credentials = Some(Token {
            token: Zeroizing::new(token.into()),
            token_secret: None,
            refresh_token: None,
            expires_in: None,
            approved_scopes: vec![],
        });
        Ok(user)
    }
    pub async fn get_access_token_response(&mut self, code: &str) -> Result<Value> {
        self.exchange(code, false).await
    }
    pub async fn get_access_token(&mut self, code: &str) -> Result<Zeroizing<String>> {
        let response = self.exchange(code, true).await?;
        response
            .get("access_token")
            .and_then(Value::as_str)
            .map(|v| Zeroizing::new(v.into()))
            .ok_or(Error::InvalidResponse)
    }
    async fn exchange(&mut self, code: &str, direct: bool) -> Result<Value> {
        let mut fields = Map::from_iter([
            ("grant_type".into(), "authorization_code".into()),
            ("client_id".into(), self.config.client_id.clone().into()),
            (
                "client_secret".into(),
                self.config.client_secret.as_str().into(),
            ),
            ("code".into(), code.into()),
            ("redirect_uri".into(), self.config.redirect.clone().into()),
        ]);
        if self.uses_pkce {
            fields.insert(
                "code_verifier".into(),
                self.request
                    .session_mut()?
                    .pull("code_verifier")
                    .unwrap_or(Value::Null),
            );
        }
        fields.extend(self.parameters.clone());
        let basic = if direct {
            self.definition.direct_token_basic_auth()
        } else {
            self.definition.token_basic_auth()
        };
        let fields = Value::Object(fields);
        let custom =
            match self
                .definition
                .get_access_token_response(&self.http, &self.config, &fields)
            {
                Some(response) => Some(response.await?),
                None => None,
            };
        let response = match custom {
            Some(response) => response,
            None => self.post(&fields, basic, true).await?,
        };
        let response = self
            .definition
            .normalize_token_response(response, self.bot_user)?;
        self.definition
            .configure_token_response(&response, &mut self.options)?;
        Ok(response)
    }
    pub async fn refresh_token(&mut self, refresh_token: &str) -> Result<Token> {
        let mut fields = serde_json::json!({"grant_type":"refresh_token","refresh_token":refresh_token,"client_id":self.config.client_id});
        if self.definition.refresh_token_includes_secret() {
            fields["client_secret"] = self.config.client_secret.as_str().into();
        }
        let custom =
            match self
                .definition
                .refresh_token_response(&self.http, &self.config, refresh_token)
            {
                Some(response) => Some(response.await?),
                None => None,
            };
        let response = match custom {
            Some(response) => response,
            None => {
                self.post(&fields, self.definition.token_basic_auth(), false)
                    .await?
            }
        };
        let response = self
            .definition
            .normalize_refresh_token_response(response, refresh_token)?;
        self.credentials(&response)
    }
    async fn post(&mut self, fields: &Value, basic: bool, code: bool) -> Result<Value> {
        let mut request = self
            .http
            .pending_request()
            .as_form()
            .redact_payload()
            .without_redirecting();
        if !code || self.definition.token_accepts_json() {
            request = request.accept_json();
        }
        if basic {
            request = request.with_basic_auth(&self.config.client_id, &self.config.client_secret);
        }
        json(
            request
                .post(&self.definition.token_endpoint(&self.options), fields)
                .await?,
        )
    }

    fn credentials(&self, response: &Value) -> Result<Token> {
        Ok(Token {
            token: Zeroizing::new(
                response
                    .get("access_token")
                    .and_then(Value::as_str)
                    .ok_or(Error::InvalidResponse)?
                    .into(),
            ),
            token_secret: None,
            refresh_token: response
                .get("refresh_token")
                .and_then(Value::as_str)
                .map(|s| Zeroizing::new(s.into())),
            expires_in: response.get("expires_in").and_then(Value::as_u64),
            approved_scopes: response
                .get("scope")
                .and_then(Value::as_str)
                .unwrap_or("")
                .split(self.definition.scope_separator())
                .map(str::to_owned)
                .collect(),
        })
    }
}
fn scalar(value: &Value) -> Result<String> {
    match value {
        Value::String(v) => Ok(v.clone()),
        Value::Bool(v) => Ok(if *v { "1" } else { "0" }.into()),
        Value::Number(v) => Ok(v.to_string()),
        Value::Null => Ok(String::new()),
        _ => Err(Error::Configuration("OAuth parameter")),
    }
}
fn random(length: usize) -> Result<String> {
    let mut out = String::with_capacity(length);
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    while out.len() < length {
        let mut bytes = [0u8; 64];
        aws_lc_rs::rand::fill(&mut bytes).map_err(|_| Error::Random)?;
        for b in bytes {
            if b < 248 {
                out.push(ALPHABET[(b % 62) as usize] as char);
                if out.len() == length {
                    break;
                }
            }
        }
    }
    Ok(out)
}
pub(super) fn json(response: openrust::http::client::Response) -> Result<Value> {
    if !response.successful() {
        return Err(Error::ProviderStatus(response.status().as_u16()));
    }
    let value: Value = response.json()?;
    if !value.is_object() && !value.is_array() {
        return Err(Error::InvalidResponse);
    }
    Ok(value)
}
