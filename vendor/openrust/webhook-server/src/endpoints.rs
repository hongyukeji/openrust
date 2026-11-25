use openrust::database::{Connection, Row, Value, query::Builder};
use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    delivery_store::{hash, number, text},
    destination::parse_target,
    extensions,
    snapshot::Snapshot,
};

const ENDPOINTS: &str = "webhook_endpoints";

#[derive(Clone)]
pub struct EndpointStore {
    connection: Connection,
    scope_key: String,
}

pub struct Endpoint {
    name: String,
    pub(crate) binding: EndpointBinding,
    pub(crate) url: String,
    pub(crate) credential: Option<String>,
    state: EndpointState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointState {
    Active,
    Paused,
    Deleted,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndpointBinding {
    pub id: String,
    pub revision: u32,
}

#[derive(PartialEq, Eq)]
pub(crate) enum EndpointPermission {
    Active,
    Paused,
    Revoked,
}

impl Endpoint {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn revision(&self) -> u32 {
        self.binding.revision
    }
    pub fn state(&self) -> EndpointState {
        self.state
    }
}

impl EndpointStore {
    pub(crate) fn new(connection: Connection, scope_key: &str) -> Self {
        Self {
            connection,
            scope_key: scope_key.into(),
        }
    }

    /// 应用先校验操作者权限，再登记新的具名端点。
    pub async fn create(
        &self,
        name: &str,
        url: &str,
        credential: Option<&str>,
    ) -> Result<Endpoint> {
        let id = self.endpoint_id(name)?;
        let url = validate_material(url, credential)?;
        let changed = self
            .connection
            .table(ENDPOINTS)
            .insert_or_ignore(&[[
                ("id".into(), id.as_str().into()),
                ("scope_key".into(), self.scope_key.as_str().into()),
                ("name".into(), name.into()),
                ("state".into(), "active".into()),
                ("revision".into(), 1.into()),
                ("url".into(), url.as_str().into()),
                (
                    "credential".into(),
                    credential.map_or(Value::Null, Into::into),
                ),
            ]
            .into_iter()
            .collect()])
            .await?;
        if changed != 1 {
            return Err(Error::EndpointConflict);
        }
        Ok(Endpoint {
            name: name.into(),
            binding: EndpointBinding { id, revision: 1 },
            url,
            credential: credential.map(str::to_owned),
            state: EndpointState::Active,
        })
    }

    pub async fn find(&self, name: &str) -> Result<Option<Endpoint>> {
        let id = self.endpoint_id(name)?;
        let row = self
            .owned(self.connection.table(ENDPOINTS), &id)
            .use_write_pdo()
            .first()
            .await?;
        row.as_ref().map(endpoint).transpose()
    }

    /// 修改目的地或具名凭据会增加配置版本。
    pub async fn replace(
        &self,
        name: &str,
        expected_revision: u32,
        url: &str,
        credential: Option<&str>,
    ) -> Result<u32> {
        let id = self.endpoint_id(name)?;
        let url = validate_material(url, credential)?;
        let revision = expected_revision
            .checked_add(1)
            .filter(|value| *value <= i32::MAX as u32)
            .ok_or(Error::EndpointConflict)?;
        let changed = self
            .owned(self.connection.table(ENDPOINTS), &id)
            .r#where("revision", "=", i64::from(expected_revision))
            .r#where("state", "<>", "deleted")
            .update(
                [
                    ("url".into(), url.into()),
                    (
                        "credential".into(),
                        credential.map_or(Value::Null, Into::into),
                    ),
                    ("revision".into(), i64::from(revision).into()),
                ]
                .into_iter()
                .collect(),
            )
            .await?;
        Self::check_changed(changed)?;
        Ok(revision)
    }

    pub async fn pause(&self, name: &str, revision: u32) -> Result<()> {
        self.transition(name, revision, "active", "paused").await
    }

    pub async fn resume(&self, name: &str, revision: u32) -> Result<()> {
        self.transition(name, revision, "paused", "active").await
    }

    pub async fn delete(&self, name: &str, revision: u32) -> Result<()> {
        let id = self.endpoint_id(name)?;
        let changed = self
            .owned(self.connection.table(ENDPOINTS), &id)
            .r#where("revision", "=", i64::from(revision))
            .r#where("state", "<>", "deleted")
            .update([("state".into(), "deleted".into())].into_iter().collect())
            .await?;
        Self::check_changed(changed)
    }

    async fn transition(&self, name: &str, revision: u32, from: &str, to: &str) -> Result<()> {
        let id = self.endpoint_id(name)?;
        let changed = self
            .owned(self.connection.table(ENDPOINTS), &id)
            .r#where("revision", "=", i64::from(revision))
            .r#where("state", "=", from)
            .update([("state".into(), to.into())].into_iter().collect())
            .await?;
        Self::check_changed(changed)
    }

    fn check_changed(changed: u64) -> Result<()> {
        if changed == 1 {
            Ok(())
        } else {
            Err(Error::EndpointConflict)
        }
    }

    fn endpoint_id(&self, name: &str) -> Result<String> {
        extensions::validate_name("endpoint.name", name)?;
        Ok(hash(
            format!("webhook-endpoint:{}:{name}", self.scope_key).as_bytes(),
        ))
    }

    fn owned<'a>(&self, builder: Builder<'a>, id: &str) -> Builder<'a> {
        builder
            .r#where("id", "=", id)
            .r#where("scope_key", "=", self.scope_key.as_str())
    }

    pub(crate) async fn permission(&self, snapshot: &Snapshot) -> Result<EndpointPermission> {
        self.permission_from(snapshot, self.connection.table(ENDPOINTS).use_write_pdo())
            .await
    }

    pub(crate) async fn permission_in(
        &self,
        snapshot: &Snapshot,
        transaction: &mut openrust::database::Transaction,
    ) -> Result<EndpointPermission> {
        self.permission_from(snapshot, transaction.table(ENDPOINTS))
            .await
    }

    pub(crate) async fn permission_for_finish(
        &self,
        snapshot: &Snapshot,
        transaction: &mut openrust::database::Transaction,
    ) -> Result<EndpointPermission> {
        let query = transaction.table(ENDPOINTS);
        self.permission_from(snapshot, query.lock_for_update())
            .await
    }

    async fn permission_from(
        &self,
        snapshot: &Snapshot,
        builder: Builder<'_>,
    ) -> Result<EndpointPermission> {
        let Some(binding) = &snapshot.endpoint else {
            return Ok(EndpointPermission::Active);
        };
        let row = self.owned(builder, &binding.id).first().await?;
        let Some(current) = row.as_ref().map(endpoint).transpose()? else {
            return Ok(EndpointPermission::Revoked);
        };
        if binding.revision != current.binding.revision
            || snapshot.signing_url != current.url
            || snapshot.credential != current.credential
        {
            return Ok(EndpointPermission::Revoked);
        }
        Ok(match current.state {
            EndpointState::Active => EndpointPermission::Active,
            EndpointState::Paused => EndpointPermission::Paused,
            EndpointState::Deleted => EndpointPermission::Revoked,
        })
    }
}

fn endpoint(row: &Row) -> Result<Endpoint> {
    let state = match text(row, "state")? {
        "active" => EndpointState::Active,
        "paused" => EndpointState::Paused,
        "deleted" => EndpointState::Deleted,
        _ => return Err(Error::InvalidRecord),
    };
    let credential = match row.get("credential") {
        Some(Value::Text(value)) => Some(value.clone()),
        Some(Value::Null) => None,
        _ => return Err(Error::InvalidRecord),
    };
    Ok(Endpoint {
        name: text(row, "name")?.into(),
        binding: EndpointBinding {
            id: text(row, "id")?.into(),
            revision: u32::try_from(number(row, "revision")?).map_err(|_| Error::InvalidRecord)?,
        },
        url: text(row, "url")?.into(),
        credential,
        state,
    })
}

fn validate_material(url: &str, credential: Option<&str>) -> Result<String> {
    if let Some(credential) = credential {
        extensions::validate_name("endpoint.credential", credential)?;
    }
    Ok(parse_target(url)?.to_string())
}
