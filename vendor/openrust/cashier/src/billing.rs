use crate::client::identifier;
use crate::store::{db, ensure_active, lock_customer, map_db, save_subscription};
use crate::{
    BillingStore, Client, Error, Invoice, Price, Proration, Provider, Result, Subscription,
};
use http::Method;
use serde_json::{Value, json};

/// Bind a trusted application identity (for example `users:42`) to one named billing account.
/// Construct only after authenticating the application user; never accept this identity from Checkout.
#[derive(Clone)]
pub struct Billable {
    client: Client,
    store: BillingStore,
    owner: String,
}
impl Billable {
    pub fn new(client: Client, store: BillingStore, owner: impl Into<String>) -> Result<Self> {
        let owner = owner.into();
        if owner.is_empty() || owner.len() > 512 || owner.chars().any(char::is_control) {
            return Err(Error::InvalidInput("billable identity"));
        }
        Ok(Self {
            client,
            store,
            owner,
        })
    }
    pub fn client(&self) -> &Client {
        &self.client
    }
    pub async fn customer_id(&self) -> Result<String> {
        self.store
            .customer(&self.client, &self.owner)
            .await?
            .ok_or(Error::NotFound)
    }
    /// Explicitly associate a pre-existing provider customer with this trusted owner.
    /// The caller must verify account ownership before calling; email matching is not authentication.
    pub async fn attach_customer(&self, id: &str) -> Result<()> {
        identifier(id)?;
        let remote = self.client.retrieve("customers", id).await?;
        if remote["id"] != id || remote["deleted"] == true {
            return Err(Error::Ownership);
        }
        self.store.attach(&self.client, &self.owner, id).await
    }
    /// Stripe's persisted customer identifier; Paddle accounts reject Stripe-only APIs.
    pub async fn stripe_id(&self) -> Result<Option<String>> {
        self.stripe()?;
        self.store.customer(&self.client, &self.owner).await
    }
    pub async fn has_stripe_id(&self) -> Result<bool> {
        Ok(self.stripe_id().await?.is_some())
    }
    pub async fn create_as_stripe_customer(&self, options: Value, key: &str) -> Result<Value> {
        self.stripe()?;
        if self.has_stripe_id().await? {
            return Err(Error::CustomerAlreadyCreated);
        }
        self.create_or_get_customer(options, key).await
    }
    pub async fn as_stripe_customer(&self, expand: &[&str]) -> Result<Value> {
        self.stripe()?;
        let id = self.customer_id().await?;
        self.client
            .call(
                Method::GET,
                &self.client.path("customers", Some(&id)),
                &json!({"expand":expand}),
                None,
            )
            .await
    }
    pub async fn create_or_get_stripe_customer(&self, options: Value, key: &str) -> Result<Value> {
        self.stripe()?;
        object(&options)?;
        if self.has_stripe_id().await? {
            let expand = match options.get("expand") {
                None => Vec::new(),
                Some(value) => value
                    .as_array()
                    .ok_or(Error::InvalidInput("expand"))?
                    .iter()
                    .map(|v| v.as_str().ok_or(Error::InvalidInput("expand")))
                    .collect::<Result<Vec<_>>>()?,
            };
            return self.as_stripe_customer(&expand).await;
        }
        self.create_as_stripe_customer(options, key).await
    }
    pub async fn update_stripe_customer(&self, options: Value, key: &str) -> Result<Value> {
        self.stripe()?;
        self.update_customer(options, key).await
    }
    pub async fn update_or_create_stripe_customer(
        &self,
        options: Value,
        key: &str,
    ) -> Result<Value> {
        self.stripe()?;
        if self.has_stripe_id().await? {
            self.update_stripe_customer(options, key).await
        } else {
            self.create_as_stripe_customer(options, key).await
        }
    }
    /// Paddle returns a provider payload; its PHP morphOne customer relationship is not emulated.
    pub async fn create_as_customer(&self, options: Value, key: &str) -> Result<Value> {
        self.paddle()?;
        if self
            .store
            .customer(&self.client, &self.owner)
            .await?
            .is_none()
        {
            options
                .get("email")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or(Error::InvalidInput("Paddle customer email"))?;
        }
        self.create_or_get_customer(options, key).await
    }
    pub async fn billing_portal_url(
        &self,
        return_url: &str,
        mut options: Value,
        key: &str,
    ) -> Result<String> {
        self.stripe()?;
        https(return_url)?;
        object(&options)?;
        options["customer"] = json!(self.customer_id().await?);
        options["return_url"] = json!(return_url);
        let session = self
            .mutate(
                Method::POST,
                "billing_portal/sessions",
                None,
                None,
                options,
                key,
            )
            .await?;
        let url = crate::types::text(&session, "url")?;
        https(&url)?;
        Ok(url)
    }
    pub async fn redirect_to_billing_portal(
        &self,
        return_url: &str,
        options: Value,
        key: &str,
    ) -> Result<openrust::http::Response> {
        let url = self.billing_portal_url(return_url, options, key).await?;
        Ok(
            openrust::http::Response::empty(http::StatusCode::FOUND).header(
                http::header::LOCATION,
                http::HeaderValue::from_str(&url).map_err(|_| Error::InvalidInput("portal URL"))?,
            ),
        )
    }
    pub async fn create_or_get_customer(&self, options: Value, key: &str) -> Result<Value> {
        if let Some(id) = self.store.customer(&self.client, &self.owner).await? {
            return self.client.retrieve("customers", &id).await;
        }
        object(&options)?;
        let value = self
            .mutate(Method::POST, "customers", None, None, options, key)
            .await?;
        let id = crate::types::text(&value, "id")?;
        self.store.attach(&self.client, &self.owner, &id).await?;
        Ok(value)
    }
    pub async fn update_customer(&self, options: Value, key: &str) -> Result<Value> {
        let id = self.customer_id().await?;
        self.mutate(
            self.update_method(),
            "customers",
            Some(&id),
            None,
            options,
            key,
        )
        .await
    }
    pub async fn billing_portal(&self, return_url: &str, key: &str) -> Result<Value> {
        https(return_url)?;
        let customer = self.customer_id().await?;
        if self.client.provider() == Provider::Stripe {
            self.mutate(
                Method::POST,
                "billing_portal/sessions",
                None,
                None,
                json!({"customer":customer,"return_url":return_url}),
                key,
            )
            .await
        } else {
            self.mutate(
                Method::POST,
                "customers",
                Some(&customer),
                Some("portal-sessions"),
                json!({}),
                key,
            )
            .await
        }
    }
    pub fn new_subscription(&self, name: &str, prices: Vec<Price>) -> Result<SubscriptionBuilder> {
        identifier(name)?;
        validate_prices(&prices)?;
        Ok(SubscriptionBuilder {
            billable: self.clone(),
            name: name.into(),
            prices,
            options: json!({}),
        })
    }
    pub async fn subscription(&self, name: &str) -> Result<Option<SubscriptionHandle>> {
        Ok(self
            .store
            .subscription(&self.client, &self.owner, name)
            .await?
            .map(|data| SubscriptionHandle {
                billable: self.clone(),
                data,
            }))
    }
    pub async fn subscription_by_id(&self, id: &str) -> Result<SubscriptionHandle> {
        identifier(id)?;
        let data = self
            .store
            .subscription_by_id(&self.client, &self.owner, id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok(SubscriptionHandle {
            billable: self.clone(),
            data,
        })
    }
    pub async fn sync_subscription(&self, id: &str) -> Result<SubscriptionHandle> {
        identifier(id)?;
        let data = self
            .store
            .sync_subscription(&self.client, &self.owner, id)
            .await?;
        Ok(SubscriptionHandle {
            billable: self.clone(),
            data,
        })
    }
    pub async fn subscribed(&self, name: &str) -> Result<bool> {
        Ok(self.subscription(name).await?.is_some_and(|s| s.valid()))
    }
    /// Hosted Checkout handles PCI-sensitive collection and SCA; never send card numbers here.
    pub async fn checkout(&self, prices: Vec<Price>, options: Value, key: &str) -> Result<Value> {
        self.checkout_mode(prices, options, key, false).await
    }
    async fn checkout_mode(
        &self,
        prices: Vec<Price>,
        mut options: Value,
        key: &str,
        recurring: bool,
    ) -> Result<Value> {
        object(&options)?;
        validate_prices(&prices)?;
        let customer = self.customer_id().await?;
        if self.client.provider() == Provider::Stripe {
            for k in ["success_url", "cancel_url"] {
                if let Some(v) = options.get(k) {
                    https(v.as_str().ok_or(Error::InvalidInput("checkout URL"))?)?;
                }
            }
            options["customer"] = json!(customer);
            options["mode"] = json!(if recurring { "subscription" } else { "payment" });
            options["line_items"] = items(&prices, Provider::Stripe);
            self.mutate(Method::POST, "checkout/sessions", None, None, options, key)
                .await
        } else {
            options["customer_id"] = json!(customer);
            options["items"] = items(&prices, Provider::Paddle);
            options["collection_mode"] = json!("automatic");
            if let Some(url) = options["checkout"]["url"].as_str() {
                https(url)?;
            }
            self.mutate(Method::POST, "transactions", None, None, options, key)
                .await
        }
    }
    pub async fn create_setup_intent(&self, options: Value, key: &str) -> Result<Value> {
        self.stripe()?;
        let mut options = options;
        object(&options)?;
        options["customer"] = json!(self.customer_id().await?);
        self.mutate(Method::POST, "setup_intents", None, None, options, key)
            .await
    }
    pub async fn payment_methods(&self, kind: &str) -> Result<Value> {
        self.stripe()?;
        identifier(kind)?;
        self.client
            .call(
                Method::GET,
                "/v1/payment_methods",
                &json!({"customer":self.customer_id().await?,"type":kind}),
                None,
            )
            .await
    }
    pub async fn add_payment_method(&self, id: &str, key: &str) -> Result<Value> {
        self.stripe()?;
        identifier(id)?;
        self.mutate(
            Method::POST,
            "payment_methods",
            Some(id),
            Some("attach"),
            json!({"customer":self.customer_id().await?}),
            key,
        )
        .await
    }
    pub async fn update_default_payment_method(&self, id: &str, key: &str) -> Result<Value> {
        self.stripe()?;
        self.owned("payment_methods", id).await?;
        self.update_customer(
            json!({"invoice_settings":{"default_payment_method":id}}),
            key,
        )
        .await
    }
    pub async fn delete_payment_method(&self, id: &str, key: &str) -> Result<Value> {
        self.stripe()?;
        self.owned("payment_methods", id).await?;
        self.mutate(
            Method::POST,
            "payment_methods",
            Some(id),
            Some("detach"),
            json!({}),
            key,
        )
        .await
    }
    pub async fn default_payment_method(&self) -> Result<Option<Value>> {
        self.stripe()?;
        let c = self
            .client
            .retrieve("customers", &self.customer_id().await?)
            .await?;
        match crate::types::object_id(&c["invoice_settings"]["default_payment_method"]) {
            Ok(id) => Ok(Some(self.owned("payment_methods", &id).await?)),
            Err(_) => Ok(None),
        }
    }
    /// A positive integer in the currency's minor unit, including zero-decimal currencies.
    pub async fn pay(
        &self,
        amount: i64,
        currency: &str,
        mut options: Value,
        key: &str,
    ) -> Result<Value> {
        self.stripe()?;
        money(amount, currency)?;
        object(&options)?;
        options["amount"] = json!(amount);
        options["currency"] = json!(currency.to_ascii_lowercase());
        options["customer"] = json!(self.customer_id().await?);
        self.mutate(Method::POST, "payment_intents", None, None, options, key)
            .await
    }
    pub async fn charge(
        &self,
        amount: i64,
        currency: &str,
        payment_method: &str,
        key: &str,
    ) -> Result<Value> {
        self.stripe()?;
        self.owned("payment_methods", payment_method).await?;
        self.pay(
            amount,
            currency,
            json!({"payment_method":payment_method,"confirm":true,"off_session":true}),
            key,
        )
        .await
    }
    pub async fn confirm_payment(&self, id: &str, options: Value, key: &str) -> Result<Value> {
        self.stripe()?;
        self.owned("payment_intents", id).await?;
        self.mutate(
            Method::POST,
            "payment_intents",
            Some(id),
            Some("confirm"),
            options,
            key,
        )
        .await
    }
    pub async fn refund(&self, id: &str, mut options: Value, key: &str) -> Result<Value> {
        object(&options)?;
        if self.client.provider() == Provider::Stripe {
            self.owned("payment_intents", id).await?;
            options["payment_intent"] = json!(id);
            options.as_object_mut().expect("validated").remove("charge");
            self.mutate(Method::POST, "refunds", None, None, options, key)
                .await
        } else {
            self.owned("transactions", id).await?;
            options["transaction_id"] = json!(id);
            options["action"] = json!("refund");
            self.mutate(Method::POST, "adjustments", None, None, options, key)
                .await
        }
    }
    pub async fn invoices(&self, parameters: Value) -> Result<Value> {
        object(&parameters)?;
        let mut parameters = parameters;
        let resource = if self.client.provider() == Provider::Stripe {
            parameters["customer"] = json!(self.customer_id().await?);
            "invoices"
        } else {
            parameters["customer_id"] = json!(self.customer_id().await?);
            "transactions"
        };
        self.client
            .api(
                Method::GET,
                &self.client.path(resource, None),
                &parameters,
                None,
            )
            .await
    }
    pub async fn transactions(&self, parameters: Value) -> Result<Value> {
        self.paddle()?;
        self.invoices(parameters).await
    }
    pub async fn find_transaction(&self, id: &str) -> Result<Value> {
        self.paddle()?;
        self.owned("transactions", id).await
    }
    pub async fn local_invoices(&self) -> Result<Vec<Invoice>> {
        self.store.invoices(&self.client, &self.owner).await
    }
    pub async fn find_invoice(&self, id: &str) -> Result<Value> {
        self.owned(
            if self.client.provider() == Provider::Stripe {
                "invoices"
            } else {
                "transactions"
            },
            id,
        )
        .await
    }
    /// Provider-hosted invoice URL/document. Invoice rendering stays with Stripe/Paddle.
    pub async fn invoice_document(&self, id: &str) -> Result<Value> {
        let invoice = self.find_invoice(id).await?;
        if self.client.provider() == Provider::Stripe {
            Ok(
                json!({"invoice_pdf":invoice["invoice_pdf"],"hosted_invoice_url":invoice["hosted_invoice_url"]}),
            )
        } else {
            self.client
                .call(
                    Method::GET,
                    &format!("{}/invoice", self.client.path("transactions", Some(id))),
                    &json!({}),
                    None,
                )
                .await
        }
    }
    pub async fn preview_invoice(&self, mut options: Value, key: &str) -> Result<Value> {
        self.stripe()?;
        object(&options)?;
        options["customer"] = json!(self.customer_id().await?);
        self.mutate(
            Method::POST,
            "invoices/create_preview",
            None,
            None,
            options,
            key,
        )
        .await
    }
    pub async fn create_invoice(&self, mut options: Value, key: &str) -> Result<Value> {
        self.stripe()?;
        object(&options)?;
        options["customer"] = json!(self.customer_id().await?);
        self.mutate(Method::POST, "invoices", None, None, options, key)
            .await
    }
    pub async fn invoice_item(
        &self,
        amount: i64,
        currency: &str,
        mut options: Value,
        key: &str,
    ) -> Result<Value> {
        self.stripe()?;
        money(amount, currency)?;
        object(&options)?;
        options["amount"] = json!(amount);
        options["currency"] = json!(currency.to_ascii_lowercase());
        options["customer"] = json!(self.customer_id().await?);
        self.mutate(Method::POST, "invoiceitems", None, None, options, key)
            .await
    }
    pub async fn pay_invoice(&self, id: &str, options: Value, key: &str) -> Result<Value> {
        self.stripe()?;
        self.owned("invoices", id).await?;
        self.mutate(
            Method::POST,
            "invoices",
            Some(id),
            Some("pay"),
            options,
            key,
        )
        .await
    }
    pub async fn finalize_invoice(&self, id: &str, key: &str) -> Result<Value> {
        self.stripe()?;
        self.owned("invoices", id).await?;
        self.mutate(
            Method::POST,
            "invoices",
            Some(id),
            Some("finalize"),
            json!({}),
            key,
        )
        .await
    }
    pub async fn void_invoice(&self, id: &str, key: &str) -> Result<Value> {
        self.stripe()?;
        self.owned("invoices", id).await?;
        self.mutate(
            Method::POST,
            "invoices",
            Some(id),
            Some("void"),
            json!({}),
            key,
        )
        .await
    }
    pub async fn tax_ids(&self) -> Result<Value> {
        self.stripe()?;
        self.customer_list("tax_ids", json!({})).await
    }
    pub async fn create_tax_id(&self, kind: &str, value: &str, key: &str) -> Result<Value> {
        self.stripe()?;
        let id = self.customer_id().await?;
        self.mutate(
            Method::POST,
            "customers",
            Some(&id),
            Some("tax_ids"),
            json!({"type":kind,"value":value}),
            key,
        )
        .await
    }
    pub async fn delete_tax_id(&self, id: &str, key: &str) -> Result<Value> {
        self.stripe()?;
        identifier(id)?;
        let customer = self.customer_id().await?;
        self.mutate(
            Method::DELETE,
            "customers",
            Some(&customer),
            Some(&format!("tax_ids/{id}")),
            json!({}),
            key,
        )
        .await
    }
    /// The unformatted customer balance in the account currency's minor unit.
    pub async fn raw_balance(&self) -> Result<i64> {
        self.balance().await
    }
    pub async fn balance(&self) -> Result<i64> {
        self.stripe()?;
        let c = self
            .client
            .retrieve("customers", &self.customer_id().await?)
            .await?;
        c["balance"].as_i64().ok_or(Error::OutcomeUnknown)
    }
    pub async fn balance_transactions(&self, parameters: Value) -> Result<Value> {
        self.stripe()?;
        object(&parameters)?;
        self.customer_list("balance_transactions", parameters).await
    }
    pub async fn apply_balance(
        &self,
        amount: i64,
        currency: &str,
        description: &str,
        key: &str,
    ) -> Result<Value> {
        self.stripe()?;
        if amount == 0 || amount == i64::MIN {
            return Err(Error::InvalidInput("balance"));
        }
        money(amount.abs(), currency)?;
        let id = self.customer_id().await?;
        self.mutate(Method::POST,"customers",Some(&id),Some("balance_transactions"),json!({"amount":amount,"currency":currency.to_ascii_lowercase(),"description":description}),key).await
    }
    pub async fn report_usage(
        &self,
        event_name: &str,
        value: u64,
        identifier: &str,
        timestamp: Option<i64>,
        key: &str,
    ) -> Result<Value> {
        self.stripe()?;
        crate::client::identifier(identifier)?;
        if event_name.is_empty() {
            return Err(Error::InvalidInput("meter event"));
        }
        let mut params = json!({"event_name":event_name,"identifier":identifier,"payload":{"stripe_customer_id":self.customer_id().await?,"value":value.to_string()}});
        if let Some(t) = timestamp {
            params["timestamp"] = json!(t);
        }
        self.mutate(
            Method::POST,
            "billing/meter_events",
            None,
            None,
            params,
            key,
        )
        .await
    }
    pub async fn addresses(&self) -> Result<Value> {
        self.paddle()?;
        self.customer_list("addresses", json!({})).await
    }
    pub async fn create_address(&self, options: Value, key: &str) -> Result<Value> {
        self.paddle()?;
        let id = self.customer_id().await?;
        self.mutate(
            Method::POST,
            "customers",
            Some(&id),
            Some("addresses"),
            options,
            key,
        )
        .await
    }
    async fn customer_list(&self, suffix: &str, params: Value) -> Result<Value> {
        let id = self.customer_id().await?;
        self.client
            .api(
                Method::GET,
                &format!("{}/{suffix}", self.client.path("customers", Some(&id))),
                &params,
                None,
            )
            .await
    }
    async fn owned(&self, resource: &str, id: &str) -> Result<Value> {
        identifier(id)?;
        let value = self.client.retrieve(resource, id).await?;
        let customer = if self.client.provider() == Provider::Stripe {
            crate::types::object_id(&value["customer"])?
        } else {
            crate::types::text(&value, "customer_id")?
        };
        if value["id"] != id || customer != self.customer_id().await? {
            return Err(Error::Ownership);
        }
        Ok(value)
    }
    async fn mutate(
        &self,
        method: Method,
        resource: &str,
        id: Option<&str>,
        suffix: Option<&str>,
        options: Value,
        key: &str,
    ) -> Result<Value> {
        object(&options)?;
        if let Some(id) = id {
            identifier(id)?;
        }
        let mut path = self.client.path(resource, id);
        if let Some(s) = suffix {
            path.push('/');
            path.push_str(s);
        }
        self.store
            .execute(&self.client, method, &path, &options, key)
            .await
    }
    fn stripe(&self) -> Result<()> {
        if self.client.provider() == Provider::Stripe {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }
    fn paddle(&self) -> Result<()> {
        if self.client.provider() == Provider::Paddle {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }
    fn update_method(&self) -> Method {
        if self.client.provider() == Provider::Stripe {
            Method::POST
        } else {
            Method::PATCH
        }
    }
}
/// Fluent subscription setup follows price, trial and Checkout vocabulary.
pub struct SubscriptionBuilder {
    billable: Billable,
    name: String,
    prices: Vec<Price>,
    options: Value,
}
impl SubscriptionBuilder {
    pub fn trial_days(mut self, days: u32) -> Result<Self> {
        if days == 0 || days > 730 {
            return Err(Error::InvalidInput("trial days"));
        }
        if self.billable.client.provider() != Provider::Stripe {
            return Err(Error::Unsupported);
        }
        self.options
            .as_object_mut()
            .expect("options")
            .remove("trial_end");
        self.options["trial_period_days"] = json!(days);
        Ok(self)
    }
    pub fn trial_until(mut self, until: i64) -> Result<Self> {
        if until <= jiff::Timestamp::now().as_second() {
            return Err(Error::InvalidInput("trial end"));
        }
        self.billable.stripe()?;
        self.options
            .as_object_mut()
            .expect("options")
            .remove("trial_period_days");
        self.options["trial_end"] = json!(until);
        Ok(self)
    }
    pub fn skip_trial(mut self) -> Result<Self> {
        self.billable.stripe()?;
        self.options
            .as_object_mut()
            .expect("options")
            .remove("trial_period_days");
        self.options["trial_end"] = json!("now");
        Ok(self)
    }
    pub fn anchor_billing_cycle_on(mut self, at: i64) -> Result<Self> {
        self.billable.stripe()?;
        if at <= jiff::Timestamp::now().as_second() {
            return Err(Error::InvalidInput("billing anchor"));
        }
        self.options["billing_cycle_anchor"] = json!(at);
        Ok(self)
    }
    pub fn with_metadata(mut self, data: Value) -> Result<Self> {
        object(&data)?;
        self.options[if self.billable.client.provider() == Provider::Stripe {
            "metadata"
        } else {
            "custom_data"
        }] = data;
        Ok(self)
    }
    pub fn quantity(mut self, quantity: u64, price: Option<&str>) -> Result<Self> {
        if quantity == 0 {
            return Err(Error::InvalidInput("quantity"));
        }
        let target = match price {
            Some(p) => p.to_owned(),
            None if self.prices.len() == 1 => self.prices[0].id.clone(),
            _ => return Err(Error::InvalidInput("specify price")),
        };
        let item = self
            .prices
            .iter_mut()
            .find(|p| p.id == target)
            .ok_or(Error::NotFound)?;
        item.quantity = Some(quantity);
        Ok(self)
    }
    pub fn with_options(mut self, options: Value) -> Result<Self> {
        object(&options)?;
        for (k, v) in options.as_object().expect("validated") {
            self.options[k] = v.clone();
        }
        Ok(self)
    }
    pub fn with_coupon(mut self, id: &str) -> Result<Self> {
        identifier(id)?;
        if self.billable.client.provider() == Provider::Stripe {
            self.options["discounts"] = json!([{"coupon":id}]);
        } else {
            self.options["discount_id"] = json!(id);
        }
        Ok(self)
    }
    pub async fn create(
        self,
        payment_method: Option<&str>,
        key: &str,
    ) -> Result<SubscriptionHandle> {
        self.billable.stripe()?;
        let mut options = self.options;
        let customer = self.billable.customer_id().await?;
        options["customer"] = json!(customer);
        options["items"] = items(&self.prices, Provider::Stripe);
        metadata(&mut options, "metadata", &self.name)?;
        options["payment_behavior"] = json!("default_incomplete");
        if let Some(pm) = payment_method {
            identifier(pm)?;
            self.billable.owned("payment_methods", pm).await?;
            options["default_payment_method"] = json!(pm);
        }
        let value = self
            .billable
            .mutate(Method::POST, "subscriptions", None, None, options, key)
            .await?;
        let id = crate::types::text(&value, "id")?;
        self.billable.sync_subscription(&id).await
    }
    pub async fn checkout(self, mut options: Value, key: &str) -> Result<Value> {
        object(&options)?;
        if self.billable.client.provider() == Provider::Stripe {
            let mut data = self.options;
            if let Some(discounts) = data.as_object_mut().expect("options").remove("discounts") {
                options["discounts"] = discounts;
            }
            metadata(&mut data, "metadata", &self.name)?;
            options["subscription_data"] = data;
        } else {
            for (k, v) in self.options.as_object().expect("validated") {
                options[k] = v.clone();
            }
            metadata(&mut options, "custom_data", &self.name)?;
        }
        self.billable
            .checkout_mode(self.prices, options, key, true)
            .await
    }
}
#[derive(Clone)]
pub struct SubscriptionHandle {
    billable: Billable,
    data: Subscription,
}
impl std::ops::Deref for SubscriptionHandle {
    type Target = Subscription;
    fn deref(&self) -> &Subscription {
        &self.data
    }
}
impl SubscriptionHandle {
    pub async fn as_stripe_subscription(&self, parameters: Value) -> Result<Value> {
        self.billable.stripe()?;
        self.as_provider_subscription(parameters).await
    }
    pub async fn as_paddle_subscription(&self, parameters: Value) -> Result<Value> {
        self.billable.paddle()?;
        self.as_provider_subscription(parameters).await
    }
    pub async fn as_provider_subscription(&self, parameters: Value) -> Result<Value> {
        object(&parameters)?;
        let value = self
            .billable
            .client
            .call(
                Method::GET,
                &self.billable.client.path("subscriptions", Some(&self.id)),
                &parameters,
                None,
            )
            .await?;
        let customer = if self.billable.client.provider() == Provider::Stripe {
            crate::types::object_id(&value["customer"])?
        } else {
            crate::types::text(&value, "customer_id")?
        };
        if customer != self.billable.customer_id().await? {
            return Err(Error::Ownership);
        }
        Ok(value)
    }
    pub async fn payment_method_update_transaction(&self) -> Result<Value> {
        self.billable.paddle()?;
        self.billable.owned("subscriptions", &self.id).await?;
        self.billable
            .client
            .call(
                Method::GET,
                &format!(
                    "{}/update-payment-method-transaction",
                    self.billable.client.path("subscriptions", Some(&self.id))
                ),
                &json!({}),
                None,
            )
            .await
    }
    pub async fn refresh(&self) -> Result<Self> {
        self.billable.sync_subscription(&self.id).await
    }
    pub async fn cancel(&self, key: &str) -> Result<Self> {
        self.cancel_at(false, key).await
    }
    pub async fn cancel_now(&self, key: &str) -> Result<Self> {
        self.cancel_at(true, key).await
    }
    async fn cancel_at(&self, now: bool, key: &str) -> Result<Self> {
        let (method, suffix, options) = match self.billable.client.provider() {
            Provider::Stripe if now => (Method::DELETE, None, json!({})),
            Provider::Stripe => (Method::POST, None, json!({"cancel_at_period_end":true})),
            Provider::Paddle => (
                Method::POST,
                Some("cancel"),
                json!({"effective_from":if now{"immediately"}else{"next_billing_period"}}),
            ),
        };
        self.change(method, suffix, options, key).await
    }
    pub async fn resume(&self, key: &str) -> Result<Self> {
        let current = self.refresh().await?;
        match self.billable.client.provider() {
            Provider::Stripe if current.paused() => {
                self.change(
                    Method::POST,
                    Some("resume"),
                    json!({"billing_cycle_anchor":"now"}),
                    key,
                )
                .await
            }
            Provider::Stripe => {
                if !current.on_grace_period() {
                    return Err(Error::InvalidInput("subscription is not on a grace period"));
                }
                self.change(
                    Method::POST,
                    None,
                    json!({"cancel_at_period_end":false,"cancel_at":null}),
                    key,
                )
                .await
            }
            Provider::Paddle if current.paused() => {
                self.change(
                    Method::POST,
                    Some("resume"),
                    json!({"effective_from":"immediately"}),
                    key,
                )
                .await
            }
            Provider::Paddle if current.on_grace_period() || current.paused_at.is_some() => {
                self.change(Method::PATCH, None, json!({"scheduled_change":null}), key)
                    .await
            }
            _ => Err(Error::InvalidInput("subscription cannot resume")),
        }
    }
    pub async fn pause(&self, now: bool, until: Option<&str>, key: &str) -> Result<Self> {
        self.billable.paddle()?;
        if let Some(t) = until {
            let timestamp = t
                .parse::<jiff::Timestamp>()
                .map_err(|_| Error::InvalidInput("resume_at"))?;
            if timestamp <= jiff::Timestamp::now() {
                return Err(Error::InvalidInput("resume_at"));
            }
        }
        self.change(Method::POST,Some("pause"),json!({"effective_from":if now{"immediately"}else{"next_billing_period"},"resume_at":until}),key).await
    }
    pub async fn charge(&self, prices: Vec<Price>, key: &str) -> Result<Value> {
        self.billable.paddle()?;
        validate_prices(&prices)?;
        self.billable.owned("subscriptions", &self.id).await?;
        self.billable.mutate(Method::POST,"subscriptions",Some(&self.id),Some("charge"),json!({"items":items(&prices,Provider::Paddle),"effective_from":"immediately","on_payment_failure":"prevent_change"}),key).await
    }
    pub async fn pause_collection(&self, behavior: &str, key: &str) -> Result<Self> {
        self.billable.stripe()?;
        if !matches!(behavior, "keep_as_draft" | "mark_uncollectible" | "void") {
            return Err(Error::InvalidInput("pause collection behavior"));
        }
        self.change(
            Method::POST,
            None,
            json!({"pause_collection":{"behavior":behavior}}),
            key,
        )
        .await
    }
    pub async fn resume_collection(&self, key: &str) -> Result<Self> {
        self.billable.stripe()?;
        self.change(Method::POST, None, json!({"pause_collection":null}), key)
            .await
    }
    pub async fn activate(&self, key: &str) -> Result<Self> {
        self.billable.paddle()?;
        self.change(Method::POST, Some("activate"), json!({}), key)
            .await
    }
    pub async fn extend_trial(&self, until: i64, key: &str) -> Result<Self> {
        self.billable.stripe()?;
        if until <= jiff::Timestamp::now().as_second() {
            return Err(Error::InvalidInput("trial end"));
        }
        self.change(Method::POST, None, json!({"trial_end":until}), key)
            .await
    }
    pub async fn swap(&self, prices: Vec<Price>, proration: Proration, key: &str) -> Result<Self> {
        validate_prices(&prices)?;
        let current = self.refresh().await?;
        let provider = self.billable.client.provider();
        let mut changed = items(&prices, provider);
        if provider == Provider::Stripe {
            let array = changed.as_array_mut().expect("items");
            for v in array.iter_mut() {
                if let Some(old) = current.items.iter().find(|i| v["price"] == i.price) {
                    v["id"] = json!(old.id);
                }
            }
            for old in &current.items {
                if !prices.iter().any(|p| p.id == old.price) {
                    array.push(json!({"id":old.id,"deleted":true}));
                }
            }
        }
        let mut options = json!({"items":changed});
        options[if provider == Provider::Stripe {
            "proration_behavior"
        } else {
            "proration_billing_mode"
        }] = json!(proration.value(provider));
        self.change(self.billable.update_method(), None, options, key)
            .await
    }
    pub async fn update_quantity(
        &self,
        quantity: u64,
        price: Option<&str>,
        proration: Proration,
        key: &str,
    ) -> Result<Self> {
        if quantity == 0 {
            return Err(Error::InvalidInput("quantity"));
        }
        let current = self.refresh().await?;
        let target = match price {
            Some(p) => p,
            None if current.items.len() == 1 => &current.items[0].price,
            _ => return Err(Error::InvalidInput("specify the price for multiple items")),
        };
        if !current.has_price(target) {
            return Err(Error::NotFound);
        }
        let prices = current
            .items
            .iter()
            .map(|i| {
                if i.price == target {
                    Price::new(&i.price, quantity)
                } else if let Some(n) = i.quantity {
                    Price::new(&i.price, n)
                } else {
                    Price::metered(&i.price)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        self.swap(prices, proration, key).await
    }
    pub async fn preview(&self, mut options: Value, key: &str) -> Result<Value> {
        object(&options)?;
        self.billable.owned("subscriptions", &self.id).await?;
        if self.billable.client.provider() == Provider::Stripe {
            options["subscription"] = json!(self.id);
            self.billable.preview_invoice(options, key).await
        } else {
            self.billable
                .mutate(
                    Method::PATCH,
                    "subscriptions",
                    Some(&self.id),
                    Some("preview"),
                    options,
                    key,
                )
                .await
        }
    }
    pub async fn update(&self, options: Value, key: &str) -> Result<Self> {
        object(&options)?;
        if options.get("customer").is_some() || options.get("customer_id").is_some() {
            return Err(Error::InvalidInput("subscription customer"));
        }
        self.change(self.billable.update_method(), None, options, key)
            .await
    }
    async fn change(
        &self,
        method: Method,
        suffix: Option<&str>,
        options: Value,
        key: &str,
    ) -> Result<Self> {
        self.billable.owned("subscriptions", &self.id).await?;
        self.billable
            .mutate(
                method,
                "subscriptions",
                Some(&self.id),
                suffix,
                options,
                key,
            )
            .await?;
        self.refresh().await
    }
}
fn object(v: &Value) -> Result<()> {
    if v.is_object() {
        Ok(())
    } else {
        Err(Error::InvalidInput("options"))
    }
}
fn metadata(v: &mut Value, key: &str, name: &str) -> Result<()> {
    if v[key].is_null() {
        v[key] = json!({});
    }
    object(&v[key])?;
    v[key]["cashier_type"] = json!(name);
    Ok(())
}
fn items(prices: &[Price], provider: Provider) -> Value {
    Value::Array(
        prices
            .iter()
            .map(|p| {
                let mut v = json!({});
                v[if provider == Provider::Stripe {
                    "price"
                } else {
                    "price_id"
                }] = json!(p.id);
                if let Some(q) = p.quantity {
                    v["quantity"] = json!(q);
                }
                v
            })
            .collect(),
    )
}
fn validate_prices(prices: &[Price]) -> Result<()> {
    if prices.is_empty() || prices.len() > 20 {
        return Err(Error::InvalidInput("prices"));
    }
    let mut seen = std::collections::HashSet::new();
    for p in prices {
        identifier(&p.id)?;
        if p.quantity == Some(0) || !seen.insert(&p.id) {
            return Err(Error::InvalidInput("duplicate price or zero quantity"));
        }
    }
    Ok(())
}
fn money(amount: i64, currency: &str) -> Result<()> {
    if amount <= 0 || currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_alphabetic()) {
        Err(Error::InvalidInput("amount or ISO currency"))
    } else {
        Ok(())
    }
}
fn https(value: &str) -> Result<()> {
    let url = url::Url::parse(value).map_err(|_| Error::InvalidInput("URL"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::InvalidInput("URL"));
    }
    Ok(())
}

// 修改请求的渠道策略与结果裁决归业务层，Store 只记录幂等状态。
pub(crate) async fn execute_operation(
    store: &BillingStore,
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
    let id = BillingStore::remote_key(client, key);
    let fingerprint = crate::webhook::hash(
        &serde_json::to_vec(&(method.as_str(), path, params))
            .map_err(|_| Error::InvalidInput("parameters"))?,
    );
    store.claim_operation(&id, &fingerprint).await?;
    let result = client.call(method, path, params, None).await;
    let state = match &result {
        Ok(_) => "done",
        Err(Error::Rejected(_) | Error::InvalidInput(_)) => "rejected",
        Err(_) => "unknown",
    };
    store.finish_operation(&id, state).await?;
    result
}
pub(crate) async fn sync_subscription(
    store: &BillingStore,
    client: &Client,
    owner: &str,
    id: &str,
) -> Result<Subscription> {
    let (client, owner, id) = (client.clone(), owner.to_owned(), id.to_owned());
    store
        .connection()
        .transaction(
            move |tx| {
                let (client, owner, id) = (client.clone(), owner.clone(), id.clone());
                Box::pin(async move {
                    let key = BillingStore::customer_key(&client, &owner);
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

// 同名订阅的业务选择规则；调用方已完成查询和解码。
pub(crate) fn select_subscription(
    mut candidates: Vec<Subscription>,
) -> Result<Option<Subscription>> {
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
            "multiple subscriptions share this name; use subscription_by_id",
        ));
    }
    if let Some(s) = active.into_iter().next() {
        return Ok(Some(s));
    }
    candidates.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
    Ok(candidates.pop())
}
