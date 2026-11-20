use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU64,
    sync::Arc,
    time::Duration,
};

use aws_sdk_dynamodb::{
    Client,
    config::{BehaviorVersion, Credentials, Region, retry::RetryConfig, timeout::TimeoutConfig},
    error::{ProvideErrorMetadata, SdkError},
    primitives::Blob,
    types::{AttributeValue, KeysAndAttributes, PutRequest, ReturnValue, WriteRequest},
};
use serde::Deserialize;
use tokio::sync::OnceCell;

use crate::contracts::cache::{Lock, LockProvider, Store};

use super::{CacheError, CacheFuture, CacheResult, DynamoDbLock, Value, backend};

type Item = HashMap<String, AttributeValue>;

const ACTIVE: &str = "attribute_exists(#key) AND #expiry > :now";
const ACTIVE_OWNER: &str = "attribute_exists(#key) AND #expiry > :now AND #value = :owner";
const SET_EXPIRY: &str = "SET #expiry = :expiry";
const BATCH_ATTEMPTS: u32 = 7;

#[derive(Deserialize)]
#[serde(default)]
pub struct DynamoDbAttributes {
    pub key: String,
    pub value: String,
    pub expiration: String,
}

impl Default for DynamoDbAttributes {
    fn default() -> Self {
        Self {
            key: "key".into(),
            value: "value".into(),
            expiration: "expires_at".into(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Options {
    key: Option<String>,
    secret: Option<String>,
    token: Option<String>,
    region: Option<String>,
    endpoint: Option<String>,
    table: String,
    attributes: Option<DynamoDbAttributes>,
}

struct Connection {
    client: OnceCell<Client>,
    config: Option<ClientConfig>,
    table: String,
    attributes: DynamoDbAttributes,
}

struct ClientConfig {
    credentials: Option<Credentials>,
    region: String,
    endpoint: Option<String>,
}

/// Stores cache values and owner-checked locks in a DynamoDB table.
#[derive(Clone)]
pub struct DynamoDbStore {
    connection: Arc<Connection>,
    prefix: String,
}

#[allow(non_snake_case, reason = "Matches Laravel's DynamoDB store API")]
impl DynamoDbStore {
    pub fn new(
        client: Client,
        table: String,
        attributes: DynamoDbAttributes,
        prefix: String,
    ) -> CacheResult<Self> {
        Self::with_connection(OnceCell::from(client), None, table, attributes, prefix)
    }

    pub(super) fn configure(options: Options, prefix: String) -> CacheResult<Self> {
        let credentials = match (options.key, options.secret) {
            (Some(key), Some(secret)) if !key.is_empty() && !secret.is_empty() => {
                Some(Credentials::new(
                    key,
                    secret,
                    options.token.filter(|token| !token.is_empty()),
                    None,
                    "cache",
                ))
            }
            _ => None,
        };
        let region = options.region.unwrap_or_else(|| "us-east-1".into());
        if region.is_empty() {
            return Err(CacheError::InvalidConfiguration);
        }
        if let Some(endpoint) = &options.endpoint {
            let uri: http::Uri = endpoint
                .parse()
                .map_err(|_| CacheError::InvalidConfiguration)?;
            if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.authority().is_none() {
                return Err(CacheError::InvalidConfiguration);
            }
        }
        Self::with_connection(
            OnceCell::new(),
            Some(ClientConfig {
                credentials,
                region,
                endpoint: options.endpoint,
            }),
            options.table,
            options.attributes.unwrap_or_default(),
            prefix,
        )
    }

    fn with_connection(
        client: OnceCell<Client>,
        config: Option<ClientConfig>,
        table: String,
        attributes: DynamoDbAttributes,
        prefix: String,
    ) -> CacheResult<Self> {
        let names = [&attributes.key, &attributes.value, &attributes.expiration];
        if table.is_empty()
            || names.iter().any(|name| name.is_empty())
            || names.into_iter().collect::<HashSet<_>>().len() != 3
        {
            return Err(CacheError::InvalidConfiguration);
        }
        Ok(Self {
            connection: Arc::new(Connection {
                client,
                config,
                table,
                attributes,
            }),
            prefix,
        })
    }

    pub fn getPrefix(&self) -> &str {
        &self.prefix
    }

    pub fn setPrefix(&mut self, prefix: String) {
        self.prefix = prefix;
    }

    pub async fn getClient(&self) -> &Client {
        self.connection
            .client
            .get_or_init(|| async {
                let config = self.connection.config.as_ref().expect("configured client");
                let version = BehaviorVersion::latest();
                let region = Region::new(config.region.clone());
                let mut builder = match &config.credentials {
                    Some(credentials) => aws_sdk_dynamodb::config::Builder::new()
                        .behavior_version(version)
                        .region(region)
                        .credentials_provider(credentials.clone()),
                    None => {
                        let shared = aws_config::defaults(version).region(region).load().await;
                        aws_sdk_dynamodb::config::Builder::from(&shared)
                    }
                };
                if let Some(endpoint) = &config.endpoint {
                    builder = builder.endpoint_url(endpoint);
                }
                Client::from_conf(
                    builder
                        .timeout_config(
                            TimeoutConfig::builder()
                                .connect_timeout(Duration::from_secs(3))
                                .operation_attempt_timeout(Duration::from_secs(5))
                                .operation_timeout(Duration::from_secs(10))
                                .build(),
                        )
                        .build(),
                )
            })
            .await
    }

    fn full_key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    fn key(&self, full_key: String) -> Item {
        HashMap::from([(
            self.connection.attributes.key.clone(),
            AttributeValue::S(full_key),
        )])
    }

    fn item(&self, key: String, value: AttributeValue, expiry: i64) -> Item {
        let attributes = &self.connection.attributes;
        let mut item = self.key(key);
        item.insert(attributes.value.clone(), value);
        item.insert(attributes.expiration.clone(), number(expiry));
        item
    }

    fn value(&self, item: &Item, now: i64) -> CacheResult<Option<Value>> {
        let attributes = &self.connection.attributes;
        if let Some(expiration) = item.get(&attributes.expiration)
            && integer(expiration)? <= now
        {
            return Ok(None);
        }
        item.get(&attributes.value).map(decode).transpose()
    }

    async fn read(&self, key: &str, consistent: bool) -> CacheResult<Option<Item>> {
        let result = self
            .getClient()
            .await
            .get_item()
            .table_name(&self.connection.table)
            .set_key(Some(self.key(self.full_key(key))))
            .consistent_read(consistent)
            .send()
            .await
            .map_err(backend)?;
        Ok(result.item)
    }

    pub(super) async fn read_lock_owner(&self, key: &str) -> CacheResult<Option<String>> {
        let Some(item) = self.read(key, true).await? else {
            return Ok(None);
        };
        self.value(&item, now())?
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(CacheError::InvalidOwner)
            })
            .transpose()
    }

    async fn write(&self, key: &str, value: &Value, expiry: i64) -> CacheResult<bool> {
        self.getClient()
            .await
            .put_item()
            .table_name(&self.connection.table)
            .set_item(Some(self.item(self.full_key(key), encode(value)?, expiry)))
            .send()
            .await
            .map_err(backend)?;
        Ok(true)
    }

    async fn write_if_missing(
        &self,
        key: &str,
        value: &Value,
        expiry: i64,
        now: i64,
    ) -> CacheResult<bool> {
        let fields = &self.connection.attributes;
        condition(
            self.getClient()
                .await
                .put_item()
                .table_name(&self.connection.table)
                .set_item(Some(self.item(self.full_key(key), encode(value)?, expiry)))
                .condition_expression("attribute_not_exists(#key) OR #expiry < :now")
                .expression_attribute_names("#key", &fields.key)
                .expression_attribute_names("#expiry", &fields.expiration)
                .expression_attribute_values(":now", number(now))
                .customize()
                .config_override(no_retry())
                .send()
                .await,
        )
    }

    async fn counter(&self, key: &str, amount: i128) -> CacheResult<i64> {
        let fields = &self.connection.attributes;
        let now = now();
        let minimum = (i128::from(i64::MIN) - amount).max(i128::from(i64::MIN));
        let maximum = (i128::from(i64::MAX) - amount).min(i128::from(i64::MAX));
        let result = self
            .getClient()
            .await
            .update_item()
            .table_name(&self.connection.table)
            .set_key(Some(self.key(self.full_key(key))))
            .condition_expression(format!("{ACTIVE} AND #value BETWEEN :min AND :max"))
            .update_expression("SET #value = #value + :amount")
            .expression_attribute_names("#key", &fields.key)
            .expression_attribute_names("#value", &fields.value)
            .expression_attribute_names("#expiry", &fields.expiration)
            .expression_attribute_values(":now", number(now))
            .expression_attribute_values(":min", number(minimum))
            .expression_attribute_values(":max", number(maximum))
            .expression_attribute_values(":amount", number(amount))
            .return_values(ReturnValue::UpdatedNew)
            .customize()
            .config_override(no_retry())
            .send()
            .await;

        match result {
            Ok(result) => counter_result(
                result
                    .attributes()
                    .and_then(|attributes| attributes.get(&fields.value))
                    .ok_or(CacheError::InvalidValue)?,
            ),
            Err(error) if conditional_failure(&error) => match self.read(key, true).await? {
                Some(item) if self.value(&item, now)?.is_some() => Err(CacheError::InvalidValue),
                _ => Err(CacheError::MissingCounter),
            },
            Err(error) => Err(backend(error)),
        }
    }

    async fn fetch_many(&self, keys: &[&str]) -> CacheResult<Vec<Option<Value>>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let mut requested = Vec::new();
        let mut unique = HashSet::new();
        for key in keys.iter().map(|key| self.full_key(key)) {
            if unique.insert(key.clone()) {
                requested.push(self.key(key));
            }
        }
        let mut values = HashMap::with_capacity(unique.len());
        let client = self.getClient().await;
        let table = &self.connection.table;
        let now = now();
        for batch in requested.chunks(100) {
            let request = KeysAndAttributes::builder()
                .set_keys(Some(batch.to_vec()))
                .consistent_read(false)
                .build()
                .map_err(backend)?;
            let mut remaining = Some(HashMap::from([(table.clone(), request)]));
            for attempt in 0..BATCH_ATTEMPTS {
                let response = client
                    .batch_get_item()
                    .set_request_items(remaining.take())
                    .send()
                    .await
                    .map_err(backend)?;
                for row in response
                    .responses()
                    .and_then(|responses| responses.get(table))
                    .into_iter()
                    .flatten()
                {
                    if let Some(AttributeValue::S(key)) = row.get(&self.connection.attributes.key) {
                        values.insert(key.clone(), self.value(row, now)?);
                    }
                }
                remaining = response.unprocessed_keys.filter(|items| !items.is_empty());
                if remaining.is_none() {
                    break;
                }
                wait_batch(attempt).await?;
            }
        }
        Ok(keys
            .iter()
            .map(|key| values.get(&self.full_key(key)).cloned().flatten())
            .collect())
    }

    async fn write_many(&self, values: &[(String, Value)], expiry: i64) -> CacheResult<bool> {
        // DynamoDB forbids duplicate keys in a batch and limits its wire size to 16 MB.
        let mut unique = HashMap::with_capacity(values.len());
        for (key, value) in values {
            unique.insert(self.full_key(key), value);
        }
        let names = &self.connection.attributes;
        let overhead = 200 + 6 * (names.key.len() + names.value.len() + names.expiration.len());
        let mut batch = Vec::with_capacity(25);
        let mut bytes = 0_usize;
        for (key, value) in unique {
            let value = encode(value)?;
            let size = overhead
                + key.len().saturating_mul(6)
                + match &value {
                    AttributeValue::S(value) => value.len().saturating_mul(6),
                    AttributeValue::B(value) => value.as_ref().len().saturating_mul(2),
                    AttributeValue::N(value) => value.len(),
                    _ => unreachable!("cache value encoding"),
                };
            if !batch.is_empty() && (batch.len() == 25 || bytes.saturating_add(size) > 15_000_000) {
                self.write_batch(std::mem::take(&mut batch)).await?;
                bytes = 0;
            }
            let put = PutRequest::builder()
                .set_item(Some(self.item(key, value, expiry)))
                .build()
                .map_err(backend)?;
            batch.push(WriteRequest::builder().put_request(put).build());
            bytes = bytes.saturating_add(size);
        }
        if !batch.is_empty() {
            self.write_batch(batch).await?;
        }
        Ok(true)
    }

    async fn write_batch(&self, batch: Vec<WriteRequest>) -> CacheResult<()> {
        let client = self.getClient().await;
        let mut remaining = Some(HashMap::from([(self.connection.table.clone(), batch)]));
        for attempt in 0..BATCH_ATTEMPTS {
            let response = client
                .batch_write_item()
                .set_request_items(remaining.take())
                .send()
                .await
                .map_err(backend)?;
            remaining = response.unprocessed_items.filter(|items| !items.is_empty());
            if remaining.is_none() {
                return Ok(());
            }
            wait_batch(attempt).await?;
        }
        unreachable!("batch loop returns an error on exhaustion")
    }

    pub async fn refreshIfOwned(
        &self,
        key: &str,
        owner: &str,
        seconds: NonZeroU64,
    ) -> CacheResult<bool> {
        let fields = &self.connection.attributes;
        let now = now();
        condition(
            self.getClient()
                .await
                .update_item()
                .table_name(&self.connection.table)
                .set_key(Some(self.key(self.full_key(key))))
                .condition_expression(ACTIVE_OWNER)
                .update_expression(SET_EXPIRY)
                .expression_attribute_names("#key", &fields.key)
                .expression_attribute_names("#expiry", &fields.expiration)
                .expression_attribute_names("#value", &fields.value)
                .expression_attribute_values(":owner", encode(&Value::from(owner))?)
                .expression_attribute_values(":now", number(now))
                .expression_attribute_values(":expiry", number(expiration(now, Some(seconds))?))
                .customize()
                .config_override(no_retry())
                .send()
                .await,
        )
    }

    pub(super) async fn delete_if_owned(&self, key: &str, owner: &str) -> CacheResult<bool> {
        let fields = &self.connection.attributes;
        condition(
            self.getClient()
                .await
                .delete_item()
                .table_name(&self.connection.table)
                .set_key(Some(self.key(self.full_key(key))))
                .condition_expression(ACTIVE_OWNER)
                .expression_attribute_names("#key", &fields.key)
                .expression_attribute_names("#expiry", &fields.expiration)
                .expression_attribute_names("#value", &fields.value)
                .expression_attribute_values(":owner", encode(&Value::from(owner))?)
                .expression_attribute_values(":now", number(now()))
                .customize()
                .config_override(no_retry())
                .send()
                .await,
        )
    }
}

impl Store for DynamoDbStore {
    fn get<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            self.read(key, false)
                .await?
                .map(|item| self.value(&item, now()))
                .transpose()
                .map(Option::flatten)
        })
    }

    fn many<'a>(&'a self, keys: &'a [&'a str]) -> CacheFuture<'a, CacheResult<Vec<Option<Value>>>> {
        Box::pin(self.fetch_many(keys))
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        ttl: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move { self.write(key, value, expiration(now(), Some(ttl))?).await })
    }

    fn put_many<'a>(
        &'a self,
        values: &'a [(String, Value)],
        ttl: NonZeroU64,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move { self.write_many(values, expiration(now(), Some(ttl))?).await })
    }

    fn add<'a>(
        &'a self,
        key: &'a str,
        value: &'a Value,
        ttl: Option<NonZeroU64>,
    ) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            let now = now();
            self.write_if_missing(key, value, expiration(now, ttl)?, now)
                .await
        })
    }

    fn pull<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<Option<Value>>> {
        Box::pin(async move {
            let result = self
                .getClient()
                .await
                .delete_item()
                .table_name(&self.connection.table)
                .set_key(Some(self.key(self.full_key(key))))
                .condition_expression("attribute_exists(#key)")
                .expression_attribute_names("#key", &self.connection.attributes.key)
                .return_values(ReturnValue::AllOld)
                .customize()
                .config_override(no_retry())
                .send()
                .await;
            match result {
                Ok(result) => result
                    .attributes()
                    .map(|item| self.value(item, now()))
                    .transpose()
                    .map(Option::flatten),
                Err(error) if conditional_failure(&error) => Ok(None),
                Err(error) => Err(backend(error)),
            }
        })
    }

    fn forever<'a>(&'a self, key: &'a str, value: &'a Value) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move { self.write(key, value, expiration(now(), None)?).await })
    }

    fn touch<'a>(&'a self, key: &'a str, ttl: NonZeroU64) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            let fields = &self.connection.attributes;
            let now = now();
            condition(
                self.getClient()
                    .await
                    .update_item()
                    .table_name(&self.connection.table)
                    .set_key(Some(self.key(self.full_key(key))))
                    .condition_expression(ACTIVE)
                    .update_expression(SET_EXPIRY)
                    .expression_attribute_names("#key", &fields.key)
                    .expression_attribute_names("#expiry", &fields.expiration)
                    .expression_attribute_values(":now", number(now))
                    .expression_attribute_values(":expiry", number(expiration(now, Some(ttl))?))
                    .send()
                    .await,
            )
        })
    }

    fn forget<'a>(&'a self, key: &'a str) -> CacheFuture<'a, CacheResult<bool>> {
        Box::pin(async move {
            self.getClient()
                .await
                .delete_item()
                .table_name(&self.connection.table)
                .set_key(Some(self.key(self.full_key(key))))
                .send()
                .await
                .map_err(backend)?;
            Ok(true)
        })
    }

    fn increment<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.counter(key, i128::from(amount)))
    }

    fn decrement<'a>(&'a self, key: &'a str, amount: i64) -> CacheFuture<'a, CacheResult<i64>> {
        Box::pin(self.counter(key, -i128::from(amount)))
    }

    fn flush(&self) -> CacheFuture<'_, CacheResult<bool>> {
        Box::pin(async { Err(CacheError::FlushUnsupported) })
    }

    fn get_prefix(&self) -> &str {
        &self.prefix
    }

    fn lock_provider(&self) -> Option<&dyn LockProvider> {
        Some(self)
    }
}

impl LockProvider for DynamoDbStore {
    fn lock(
        &self,
        name: &str,
        lease: Option<Duration>,
        owner: Option<&str>,
    ) -> CacheResult<Box<dyn Lock>> {
        Ok(Box::new(DynamoDbLock::new(
            self.clone(),
            name.into(),
            lease,
            owner,
        )?))
    }

    fn restore_lock(&self, name: &str, owner: &str) -> CacheResult<Box<dyn Lock>> {
        self.lock(name, None, Some(owner))
    }
}

fn now() -> i64 {
    crate::support::timestamp_seconds(jiff::Timestamp::now())
}

fn expiration(now: i64, ttl: Option<NonZeroU64>) -> CacheResult<i64> {
    if let Some(ttl) = ttl {
        return now
            .checked_add(i64::try_from(ttl.get()).map_err(|_| CacheError::InvalidValue)?)
            .ok_or(CacheError::InvalidValue);
    }
    jiff::Timestamp::from_second(now)
        .and_then(|time| {
            time.to_zoned(jiff::tz::TimeZone::UTC)
                .checked_add(jiff::Span::new().months(60))
        })
        .map(|time| crate::support::timestamp_seconds(time.timestamp()))
        .map_err(|_| CacheError::InvalidValue)
}

fn number(value: impl ToString) -> AttributeValue {
    AttributeValue::N(value.to_string())
}

fn integer(value: &AttributeValue) -> CacheResult<i64> {
    value
        .as_n()
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or(CacheError::InvalidValue)
}

fn counter_result(value: &AttributeValue) -> CacheResult<i64> {
    let value = value.as_n().map_err(|_| CacheError::InvalidValue)?;
    value
        .parse::<i64>()
        .or_else(|_| value.parse::<f64>().map(|value| value as i64))
        .map_err(|_| CacheError::InvalidValue)
}

fn encode(value: &Value) -> CacheResult<AttributeValue> {
    match value {
        Value::Json(serde_json::Value::Number(value)) => Ok(number(value)),
        Value::Json(value) => Ok(AttributeValue::S(
            serde_json::to_string(value).map_err(backend)?,
        )),
        Value::Bytes(value) => Ok(AttributeValue::B(Blob::new(value.clone()))),
    }
}

fn decode(value: &AttributeValue) -> CacheResult<Value> {
    match value {
        AttributeValue::S(value) => serde_json::from_str(value)
            .map(Value::Json)
            .map_err(backend),
        AttributeValue::N(value) => serde_json::from_str::<serde_json::Number>(value)
            .map(|number| Value::Json(serde_json::Value::Number(number)))
            .map_err(backend),
        AttributeValue::B(value) => Ok(Value::Bytes(value.as_ref().to_vec())),
        _ => Err(CacheError::InvalidValue),
    }
}

fn conditional_failure<E: ProvideErrorMetadata>(error: &SdkError<E>) -> bool {
    error
        .as_service_error()
        .and_then(ProvideErrorMetadata::code)
        == Some("ConditionalCheckFailedException")
}

fn condition<T, E>(result: Result<T, SdkError<E>>) -> CacheResult<bool>
where
    E: std::error::Error + ProvideErrorMetadata + Send + Sync + 'static,
{
    match result {
        Ok(_) => Ok(true),
        Err(error) if conditional_failure(&error) => Ok(false),
        Err(error) => Err(backend(error)),
    }
}

fn no_retry() -> aws_sdk_dynamodb::config::Builder {
    aws_sdk_dynamodb::config::Builder::new().retry_config(RetryConfig::disabled())
}

async fn wait_batch(attempt: u32) -> CacheResult<()> {
    if attempt + 1 == BATCH_ATTEMPTS {
        return Err(backend(std::io::Error::other(
            "DynamoDB did not process the complete cache batch",
        )));
    }
    let limit = 10_u64 << attempt;
    tokio::time::sleep(Duration::from_millis(fastrand::u64(limit..=2 * limit))).await;
    Ok(())
}
