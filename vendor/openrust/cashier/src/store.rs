use crate::webhook::hash;
use crate::{Client, Error, Invoice, Provider, Result, Subscription, VerifiedEvent};
use openrust::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value as SqlValue,
};
use serde_json::Value;
use std::sync::Arc;
type Record = std::collections::BTreeMap<String, SqlValue>;
#[derive(Clone)]
pub struct BillingStore {
    connection: Connection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebhookOutcome {
    Processed,
    Duplicate,
}
impl BillingStore {
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }
    pub(crate) fn customer_key(client: &Client, owner: &str) -> String {
        hash(format!("{}\0{owner}", client.scope()).as_bytes())
    }
    pub(crate) fn remote_key(client: &Client, id: &str) -> String {
        hash(format!("{}\0{id}", client.scope()).as_bytes())
    }
    pub async fn customer(&self, client: &Client, owner: &str) -> Result<Option<String>> {
        let row = self
            .connection
            .table("cashier_customers")
            .r#where("id", "=", Self::customer_key(client, owner))
            .first()
            .await
            .map_err(map_db)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.get("status") != Some(&SqlValue::from("active")) {
            return Err(Error::NotFound);
        }
        match row.get("remote_id") {
            Some(SqlValue::Text(s)) => Ok(Some(s.clone())),
            _ => Err(Error::Database),
        }
    }
    pub(crate) async fn attach(&self, client: &Client, owner: &str, remote: &str) -> Result<()> {
        let client = client.clone();
        let owner = owner.to_owned();
        let remote = remote.to_owned();
        self.connection
            .transaction(
                move |tx| {
                    let (client, owner, remote) = (client.clone(), owner.clone(), remote.clone());
                    Box::pin(async move {
                        let key = Self::customer_key(&client, &owner);
                        let candidate = record([
                            ("id", key.clone().into()),
                            ("remote_key", Self::remote_key(&client, &remote).into()),
                            ("remote_id", remote.clone().into()),
                            ("status", "active".into()),
                        ]);
                        tx.table("cashier_customers")
                            .insert_or_ignore(&[candidate])
                            .await?;
                        let row = tx
                            .table("cashier_customers")
                            .r#where("id", "=", key.clone())
                            .lock_for_update()
                            .first()
                            .await?
                            .ok_or_else(|| db(Error::Ownership))?;
                        if row.get("status") == Some(&SqlValue::from("deleted")) {
                            tx.table("cashier_customers")
                                .r#where("id", "=", key)
                                .update(record([
                                    ("remote_key", Self::remote_key(&client, &remote).into()),
                                    ("remote_id", remote.into()),
                                    ("status", "active".into()),
                                ]))
                                .await?;
                        } else if row.get("remote_id") != Some(&SqlValue::from(remote)) {
                            return Err(db(Error::Ownership));
                        }
                        Ok(())
                    })
                },
                1,
            )
            .await
            .map_err(map_db)
    }
    /// Paddle mutations are claimed durably before sending. Reusing a key never sends twice,
    /// including after a crash. Reconcile via the provider before starting a new operation.
    pub async fn execute(
        &self,
        client: &Client,
        method: http::Method,
        path: &str,
        params: &Value,
        key: &str,
    ) -> Result<Value> {
        crate::client::operation_key(key)?;
        if client.provider() == Provider::Stripe {
            return client.call(method, path, params, Some(key)).await;
        }
        let id = Self::remote_key(client, key);
        let fingerprint = hash(
            &serde_json::to_vec(&(method.as_str(), path, params))
                .map_err(|_| Error::InvalidInput("parameters"))?,
        );
        let n = self
            .connection
            .table("cashier_operations")
            .insert_or_ignore(&[record([
                ("id", id.clone().into()),
                ("fingerprint", fingerprint.clone().into()),
                ("status", "pending".into()),
            ])])
            .await
            .map_err(map_db)?;
        if n == 0 {
            let old = self
                .connection
                .table("cashier_operations")
                .r#where("id", "=", id)
                .value("fingerprint")
                .await
                .map_err(map_db)?;
            return Err(if old == fingerprint.into() {
                Error::OutcomeUnknown
            } else {
                Error::EventConflict
            });
        }
        let result = client.call(method, path, params, None).await;
        let state = match &result {
            Ok(_) => "done",
            Err(Error::Rejected(_) | Error::InvalidInput(_)) => "rejected",
            Err(_) => "unknown",
        };
        self.connection
            .table("cashier_operations")
            .r#where("id", "=", id)
            .update(record([("status", state.into())]))
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        result
    }
    pub async fn operationStatus(&self, client: &Client, key: &str) -> Result<Option<String>> {
        match self
            .connection
            .table("cashier_operations")
            .r#where("id", "=", Self::remote_key(client, key))
            .value("status")
            .await
            .map_err(map_db)?
        {
            SqlValue::Null => Ok(None),
            SqlValue::Text(s) => Ok(Some(s)),
            _ => Err(Error::Database),
        }
    }
    pub async fn subscription(
        &self,
        client: &Client,
        owner: &str,
        name: &str,
    ) -> Result<Option<Subscription>> {
        match self.customer(client, owner).await {
            Ok(Some(_)) => {}
            Ok(None) | Err(Error::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        }
        let rows = self
            .connection
            .table("cashier_subscriptions")
            .r#where("customer_key", "=", Self::customer_key(client, owner))
            .r#where("name", "=", name)
            .get()
            .await
            .map_err(map_db)?;
        let mut candidates = rows
            .iter()
            .map(|r| decode(r.get("payload")))
            .collect::<Result<Vec<Subscription>>>()?;
        let active = candidates
            .iter()
            .filter(|s| {
                !matches!(
                    s.status.as_str(),
                    "canceled" | "cancelled" | "incomplete_expired"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(Error::InvalidInput(
                "multiple subscriptions share this name; use subscriptionById",
            ));
        }
        if let Some(s) = active.into_iter().next() {
            return Ok(Some(s));
        }
        candidates.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        Ok(candidates.pop())
    }
    pub async fn subscriptionById(
        &self,
        client: &Client,
        owner: &str,
        id: &str,
    ) -> Result<Option<Subscription>> {
        match self.customer(client, owner).await {
            Ok(Some(_)) => {}
            Ok(None) | Err(Error::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        }
        let row = self
            .connection
            .table("cashier_subscriptions")
            .r#where("id", "=", Self::remote_key(client, id))
            .r#where("customer_key", "=", Self::customer_key(client, owner))
            .first()
            .await
            .map_err(map_db)?;
        row.as_ref().map(|r| decode(r.get("payload"))).transpose()
    }
    pub async fn invoices(&self, client: &Client, owner: &str) -> Result<Vec<Invoice>> {
        self.connection
            .table("cashier_invoices")
            .r#where("customer_key", "=", Self::customer_key(client, owner))
            .get()
            .await
            .map_err(map_db)?
            .iter()
            .map(|r| decode(r.get("payload")))
            .collect()
    }
    /// Fetch the current provider state under the same customer lock used by webhooks.
    /// This prevents stale webhook snapshots or overlapping mutations from regressing local state.
    pub async fn syncSubscription(
        &self,
        client: &Client,
        owner: &str,
        id: &str,
    ) -> Result<Subscription> {
        let (client, owner, id) = (client.clone(), owner.to_owned(), id.to_owned());
        self.connection
            .transaction(
                move |tx| {
                    let (client, owner, id) = (client.clone(), owner.clone(), id.clone());
                    Box::pin(async move {
                        let key = Self::customer_key(&client, &owner);
                        let remote = lock_customer(tx, &key).await?;
                        ensure_active(tx, &key).await?;
                        let v = client.retrieve("subscriptions", &id).await.map_err(db)?;
                        let sub = Subscription::parse(client.provider(), &v).map_err(db)?;
                        if sub.customer != remote || sub.id != id {
                            return Err(db(Error::Ownership));
                        }
                        save_subscription(tx, &client, &key, &sub).await?;
                        Ok(sub)
                    })
                },
                1,
            )
            .await
            .map_err(map_db)
    }
    /// Verify first, then atomically reconcile provider state, run application SQL, and mark done.
    /// Handlers must use the supplied transaction; external effects need a transactional outbox.
    pub async fn process(
        &self,
        client: &Client,
        event: &VerifiedEvent,
        handler: impl for<'a> Fn(
            &'a mut Transaction,
            &'a VerifiedEvent,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<WebhookOutcome> {
        if event.scope != client.scope() {
            return Err(Error::Ownership);
        }
        let (client, event, handler) = (client.clone(), event.clone(), Arc::new(handler));
        self.connection
            .transaction(
                move |tx| {
                    let (client, mut event, handler) =
                        (client.clone(), event.clone(), handler.clone());
                    Box::pin(async move {
                        let key = Self::remote_key(&client, &event.id);
                        tx.table("cashier_events")
                            .upsert(
                                &[record([
                                    ("id", key.clone().into()),
                                    ("fingerprint", event.fingerprint.clone().into()),
                                    ("status", "pending".into()),
                                ])],
                                &["id"],
                                Some(&["id"]),
                            )
                            .await?;
                        let row = tx
                            .table("cashier_events")
                            .r#where("id", "=", key.clone())
                            .lock_for_update()
                            .first()
                            .await?
                            .ok_or_else(|| db(Error::Database))?;
                        if row.get("fingerprint") != Some(&event.fingerprint.clone().into()) {
                            return Err(db(Error::EventConflict));
                        }
                        if row.get("status") == Some(&SqlValue::from("done")) {
                            return Ok(WebhookOutcome::Duplicate);
                        }
                        let stripe = client.provider() == Provider::Stripe;
                        let resource = if (stripe
                            && event.kind.starts_with("customer.subscription."))
                            || (!stripe && event.kind.starts_with("subscription."))
                        {
                            Some("subscriptions")
                        } else if stripe && event.kind.starts_with("invoice.") {
                            Some("invoices")
                        } else if !stripe && event.kind.starts_with("transaction.") {
                            Some("transactions")
                        } else {
                            None
                        };
                        if let Some(resource) = resource {
                            let id = crate::types::text(&event.payload, "id").map_err(db)?;
                            let remote = if stripe {
                                crate::types::object_id(&event.payload["customer"])
                            } else {
                                crate::types::text(&event.payload, "customer_id")
                            }
                            .map_err(db)?;
                            let customer = tx
                                .table("cashier_customers")
                                .r#where("remote_key", "=", Self::remote_key(&client, &remote))
                                .value("id")
                                .await?;
                            let SqlValue::Text(customer) = customer else {
                                return Err(db(Error::Ownership));
                            };
                            if lock_customer(tx, &customer).await? != remote {
                                return Err(db(Error::Ownership));
                            }
                            ensure_active(tx, &customer).await?;
                            let current = client.retrieve(resource, &id).await.map_err(db)?;
                            if resource == "subscriptions" {
                                let sub =
                                    Subscription::parse(client.provider(), &current).map_err(db)?;
                                if sub.customer != remote || sub.id != id {
                                    return Err(db(Error::Ownership));
                                }
                                save_subscription(tx, &client, &customer, &sub).await?;
                                event.subscription = Some(sub);
                            } else {
                                let invoice =
                                    Invoice::parse(client.provider(), &current).map_err(db)?;
                                if invoice.customer != remote || invoice.id != id {
                                    return Err(db(Error::Ownership));
                                }
                                tx.table("cashier_invoices")
                                    .upsert(
                                        &[record([
                                            ("id", Self::remote_key(&client, &id).into()),
                                            ("customer_key", customer.into()),
                                            (
                                                "payload",
                                                serde_json::to_string(&invoice)
                                                    .map_err(|_| db(Error::Database))?
                                                    .into(),
                                            ),
                                        ])],
                                        &["id"],
                                        Some(&["payload"]),
                                    )
                                    .await?;
                                event.invoice = Some(invoice);
                            }
                        }
                        if stripe && event.kind == "customer.deleted" {
                            let remote = crate::types::text(&event.payload, "id").map_err(db)?;
                            let customer = tx
                                .table("cashier_customers")
                                .r#where("remote_key", "=", Self::remote_key(&client, &remote))
                                .value("id")
                                .await?;
                            let SqlValue::Text(customer) = customer else {
                                return Err(db(Error::Ownership));
                            };
                            if lock_customer(tx, &customer).await? != remote {
                                return Err(db(Error::Ownership));
                            }
                            let current =
                                client.retrieve("customers", &remote).await.map_err(db)?;
                            if current["id"] != remote || current["deleted"] != true {
                                return Err(db(Error::OutcomeUnknown));
                            }
                            tx.table("cashier_customers")
                                .r#where("id", "=", customer.clone())
                                .update(record([("status", "deleted".into())]))
                                .await?;
                            for row in tx
                                .table("cashier_subscriptions")
                                .r#where("customer_key", "=", customer.clone())
                                .get()
                                .await?
                            {
                                let mut sub: Subscription =
                                    decode(row.get("payload")).map_err(db)?;
                                sub.status = "canceled".into();
                                sub.ends_at = Some(jiff::Timestamp::now().as_second());
                                sub.trial_ends_at = None;
                                save_subscription(tx, &client, &customer, &sub).await?;
                            }
                            event.customer_deleted = true;
                        }
                        handler(tx, &event).await?;
                        tx.table("cashier_events")
                            .r#where("id", "=", key)
                            .update(record([("status", "done".into())]))
                            .await?;
                        Ok(WebhookOutcome::Processed)
                    })
                },
                1,
            )
            .await
            .map_err(map_db)
    }
    pub async fn handleWebhook(
        &self,
        client: &Client,
        event: &VerifiedEvent,
    ) -> Result<WebhookOutcome> {
        self.process(client, event, |_, _| Box::pin(async { Ok(()) }))
            .await
    }
}
async fn lock_customer(tx: &mut Transaction, key: &str) -> DatabaseResult<String> {
    tx.table("cashier_customers")
        .r#where("id", "=", key)
        .update(record([("id", key.into())]))
        .await?;
    match tx
        .table("cashier_customers")
        .r#where("id", "=", key)
        .lock_for_update()
        .value("remote_id")
        .await?
    {
        SqlValue::Text(s) => Ok(s),
        _ => Err(db(Error::Ownership)),
    }
}
async fn ensure_active(tx: &mut Transaction, key: &str) -> DatabaseResult<()> {
    let status = tx
        .table("cashier_customers")
        .r#where("id", "=", key)
        .lock_for_update()
        .value("status")
        .await?;
    if status != SqlValue::from("active") {
        return Err(db(Error::Ownership));
    }
    Ok(())
}
async fn save_subscription(
    tx: &mut Transaction,
    client: &Client,
    customer: &str,
    sub: &Subscription,
) -> DatabaseResult<()> {
    tx.table("cashier_subscriptions")
        .upsert(
            &[record([
                ("id", BillingStore::remote_key(client, &sub.id).into()),
                ("customer_key", customer.into()),
                ("name", sub.name.clone().into()),
                (
                    "payload",
                    serde_json::to_string(sub)
                        .map_err(|_| db(Error::Database))?
                        .into(),
                ),
            ])],
            &["id"],
            Some(&["name", "payload"]),
        )
        .await?;
    Ok(())
}
fn decode<T: serde::de::DeserializeOwned>(value: Option<&SqlValue>) -> Result<T> {
    let Some(SqlValue::Text(s)) = value else {
        return Err(Error::Database);
    };
    serde_json::from_str(s).map_err(|_| Error::Database)
}
fn record<const N: usize>(v: [(&str, SqlValue); N]) -> Record {
    v.into_iter().map(|(k, v)| (k.into(), v)).collect()
}
fn db(e: Error) -> DatabaseError {
    DatabaseError::Embedding(Box::new(e))
}
fn map_db(e: DatabaseError) -> Error {
    match e {
        DatabaseError::Embedding(e) => e.downcast::<Error>().map(|e| *e).unwrap_or(Error::Database),
        _ => Error::Database,
    }
}
