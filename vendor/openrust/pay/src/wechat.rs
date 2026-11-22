use crate::{
    Amount, Error, Order, Payment, Refund, Result, Scene, VerifiedNotification,
    crypto::{self, SigningKey, required},
};
use http::{HeaderMap, HeaderValue, Method};
use openrust::http::client::{Factory, Response};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use zeroize::Zeroizing;

pub(crate) struct Wechat {
    app_id: String,
    mch_id: String,
    serial: String,
    key: SigningKey,
    api_v3_key: Zeroizing<String>,
    keys: BTreeMap<String, Vec<u8>>,
    preferred_key: String,
    endpoint: url::Url,
    http: Arc<Factory>,
}
impl Wechat {
    pub fn new(profile: &Value, http: Arc<Factory>) -> Result<Self> {
        let api_v3_key = required(profile, "api_v3_key")?;
        if api_v3_key.len() != 32 {
            return Err(Error::Configuration("api_v3_key"));
        }
        let preferred_key = required(profile, "public_key_id")?.to_owned();
        if !preferred_key.starts_with("PUB_KEY_ID_") {
            return Err(Error::Configuration("public_key_id"));
        }
        let mut keys = BTreeMap::new();
        keys.insert(
            preferred_key.clone(),
            crypto::public_key(&crypto::pem(profile, "public_key", "public_key_path")?)?,
        );
        if let Some(extra) = profile.get("public_keys") {
            for (id, pem) in extra
                .as_object()
                .ok_or(Error::Configuration("public_keys"))?
            {
                if !id.starts_with("PUB_KEY_ID_") {
                    return Err(Error::Configuration("public_keys"));
                }
                if keys.contains_key(id) {
                    return Err(Error::Configuration("duplicate public key id"));
                }
                keys.insert(
                    id.clone(),
                    crypto::public_key(pem.as_str().ok_or(Error::Configuration("public_keys"))?)?,
                );
            }
        }
        let mch_id = required(profile, "mch_id")?.to_owned();
        let app_id = required(profile, "app_id")?.to_owned();
        let serial = required(profile, "serial_no")?.to_owned();
        if [&mch_id, &app_id, &serial, &preferred_key]
            .iter()
            .any(|s| !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
        {
            return Err(Error::Configuration("merchant identity"));
        }
        let endpoint = crypto::endpoint(profile, "https://api.mch.weixin.qq.com/")?;
        if endpoint.path() != "/" {
            return Err(Error::Configuration("endpoint"));
        }
        Ok(Self {
            app_id,
            mch_id,
            serial,
            key: SigningKey::new(&crypto::pem(profile, "private_key", "private_key_path")?)?,
            api_v3_key: Zeroizing::new(api_v3_key.to_owned()),
            keys,
            preferred_key,
            endpoint,
            http,
        })
    }
    fn verify(&self, headers: &HeaderMap, body: &[u8]) -> Result<()> {
        let header = |name| -> Result<&str> {
            let mut values = headers.get_all(name).iter();
            let first = values.next().ok_or(Error::Verification)?;
            if values.next().is_some() {
                return Err(Error::Verification);
            }
            first.to_str().map_err(|_| Error::Verification)
        };
        let timestamp = header("wechatpay-timestamp")?;
        let seconds: i64 = timestamp.parse().map_err(|_| Error::Verification)?;
        if jiff::Timestamp::now().as_second().abs_diff(seconds) > 300 {
            return Err(Error::Verification);
        }
        let nonce = header("wechatpay-nonce")?;
        if nonce.is_empty() || nonce.len() > 256 {
            return Err(Error::Verification);
        }
        let key = self
            .keys
            .get(header("wechatpay-serial")?)
            .ok_or(Error::Verification)?;
        let mut message = format!("{timestamp}\n{nonce}\n").into_bytes();
        message.extend_from_slice(body);
        message.push(b'\n');
        crypto::verify(key, &message, header("wechatpay-signature")?)
    }
    async fn request(&self, method: Method, path: &str, payload: Option<&Value>) -> Result<Value> {
        let body = payload
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| Error::Protocol)?
            .unwrap_or_default();
        let timestamp = jiff::Timestamp::now().as_second().to_string();
        let nonce = crypto::nonce()?;
        let mut message = format!("{method}\n{path}\n{timestamp}\n{nonce}\n").into_bytes();
        message.extend_from_slice(&body);
        message.push(b'\n');
        let signature = self.key.sign(&message)?;
        let auth = format!(
            "WECHATPAY2-SHA256-RSA2048 mchid=\"{}\",nonce_str=\"{nonce}\",timestamp=\"{timestamp}\",serial_no=\"{}\",signature=\"{signature}\"",
            self.mch_id, self.serial
        );
        let mut headers = HeaderMap::new();
        let mut authorization = HeaderValue::from_str(&auth).map_err(|_| Error::Cryptography)?;
        authorization.set_sensitive(true);
        headers.insert("authorization", authorization);
        headers.insert(
            "wechatpay-serial",
            HeaderValue::from_str(&self.preferred_key)
                .map_err(|_| Error::Configuration("public_key_id"))?,
        );
        let url = self.endpoint.join(path).map_err(|_| Error::Protocol)?;
        let response: Response = self
            .http
            .pendingRequest()
            .redactPayload()
            .withoutRedirecting()
            .timeout(Duration::from_secs(15))
            .maxResponseBytes(1024 * 1024)
            .acceptJson()
            .withHeaders(headers)
            .withBody(body, HeaderValue::from_static("application/json"))
            .send(method, url.as_str())
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        self.verify(
            response.headers(),
            &response.body().map_err(|_| Error::OutcomeUnknown)?,
        )
        .map_err(|_| Error::OutcomeUnknown)?;
        if !response.successful() {
            return Err(if response.status().is_server_error() {
                Error::OutcomeUnknown
            } else {
                Error::Rejected
            });
        }
        if response.status() == http::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        response.json().map_err(|_| Error::OutcomeUnknown)
    }
    pub async fn pay(&self, scene: Scene, order: &Order) -> Result<Payment> {
        order.validate()?;
        let suffix = match scene {
            Scene::Scan => "native",
            Scene::Jsapi => "jsapi",
            Scene::Wap => "h5",
            Scene::App => "app",
            _ => return Err(Error::InvalidInput("WeChat payment scene")),
        };
        let mut body = json!({"appid":self.app_id,"mchid":self.mch_id,"description":order.description,"out_trade_no":order.out_trade_no,"notify_url":order.notify_url,"amount":{"total":order.amount.fen(),"currency":"CNY"}});
        if scene == Scene::Jsapi {
            body["payer"] = json!({"openid":order.openid.as_deref().filter(|s| !s.is_empty()).ok_or(Error::InvalidInput("openid"))?});
        }
        if scene == Scene::Wap {
            body["scene_info"] = json!({"payer_client_ip":order.client_ip.ok_or(Error::InvalidInput("client_ip"))?.to_string(),"h5_info":{"type":"Wap"}});
        }
        let result = self
            .request(
                Method::POST,
                &format!("/v3/pay/transactions/{suffix}"),
                Some(&body),
            )
            .await?;
        let value = |key| {
            result[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .ok_or(Error::OutcomeUnknown)
        };
        match scene {
            Scene::Scan => Ok(Payment::QrCode(value("code_url")?)),
            Scene::Wap => Ok(Payment::Redirect(value("h5_url")?)),
            Scene::Jsapi | Scene::App => {
                let prepay = value("prepay_id")?;
                let nonce = crypto::nonce()?;
                let timestamp = jiff::Timestamp::now().as_second().to_string();
                if scene == Scene::Jsapi {
                    let package = format!("prepay_id={prepay}");
                    let signed = self.key.sign(
                        format!("{}\n{timestamp}\n{nonce}\n{package}\n", self.app_id).as_bytes(),
                    )?;
                    Ok(Payment::Jsapi(
                        json!({"appId":self.app_id,"timeStamp":timestamp,"nonceStr":nonce,"package":package,"signType":"RSA","paySign":signed}),
                    ))
                } else {
                    let signed = self.key.sign(
                        format!("{}\n{timestamp}\n{nonce}\n{prepay}\n", self.app_id).as_bytes(),
                    )?;
                    Ok(Payment::WechatApp(
                        json!({"appid":self.app_id,"partnerid":self.mch_id,"prepayid":prepay,"package":"Sign=WXPay","noncestr":nonce,"timestamp":timestamp,"sign":signed}),
                    ))
                }
            }
            _ => Err(Error::InvalidInput("scene")),
        }
    }
    pub async fn find(&self, order: &str) -> Result<Value> {
        crate::types::identifier(order)?;
        let result = self
            .request(
                Method::GET,
                &format!(
                    "/v3/pay/transactions/out-trade-no/{order}?mchid={}",
                    self.mch_id
                ),
                None,
            )
            .await?;
        if result["out_trade_no"] != order
            || result["mchid"] != self.mch_id
            || result["appid"] != self.app_id
        {
            return Err(Error::OutcomeUnknown);
        }
        Ok(result)
    }
    pub async fn close(&self, order: &str) -> Result<Value> {
        crate::types::identifier(order)?;
        self.request(
            Method::POST,
            &format!("/v3/pay/transactions/out-trade-no/{order}/close"),
            Some(&json!({"mchid":self.mch_id})),
        )
        .await
    }
    pub async fn refund(&self, refund: &Refund) -> Result<Value> {
        refund.validate()?;
        let mut body = json!({"out_trade_no":refund.out_trade_no,"out_refund_no":refund.out_refund_no,"amount":{"refund":refund.amount.fen(),"total":refund.total.fen(),"currency":"CNY"}});
        if let Some(reason) = &refund.reason {
            body["reason"] = json!(reason);
        }
        let result = self
            .request(Method::POST, "/v3/refund/domestic/refunds", Some(&body))
            .await?;
        if result["out_trade_no"] != refund.out_trade_no
            || result["out_refund_no"] != refund.out_refund_no
        {
            return Err(Error::OutcomeUnknown);
        }
        Ok(result)
    }
    pub async fn find_refund(&self, order: &str, refund: &str) -> Result<Value> {
        crate::types::identifier(order)?;
        crate::types::identifier(refund)?;
        let result = self
            .request(
                Method::GET,
                &format!("/v3/refund/domestic/refunds/{refund}"),
                None,
            )
            .await?;
        if result["out_trade_no"] != order || result["out_refund_no"] != refund {
            return Err(Error::OutcomeUnknown);
        }
        Ok(result)
    }
    pub fn callback(&self, headers: &HeaderMap, body: &[u8]) -> Result<VerifiedNotification> {
        if body.len() > 1024 * 1024 {
            return Err(Error::Protocol);
        }
        self.verify(headers, body)?;
        let envelope: Value = serde_json::from_slice(body).map_err(|_| Error::Protocol)?;
        if envelope["event_type"] != "TRANSACTION.SUCCESS"
            || envelope["resource_type"] != "encrypt-resource"
            || envelope["resource"]["algorithm"] != "AEAD_AES_256_GCM"
            || envelope["resource"]["original_type"] != "transaction"
        {
            return Err(Error::Protocol);
        }
        let resource = &envelope["resource"];
        let get = |key| resource[key].as_str().ok_or(Error::Protocol);
        let decrypted = crypto::decrypt(
            self.api_v3_key.as_bytes(),
            get("nonce")?,
            get("associated_data")?,
            get("ciphertext")?,
        )?;
        let transaction: Value = serde_json::from_slice(&decrypted).map_err(|_| Error::Protocol)?;
        if transaction["appid"] != self.app_id
            || transaction["mchid"] != self.mch_id
            || transaction["trade_state"] != "SUCCESS"
            || transaction["amount"]["currency"] != "CNY"
        {
            return Err(Error::OrderMismatch);
        }
        let get = |key| {
            transaction[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or(Error::Protocol)
        };
        let out_trade_no = get("out_trade_no")?.to_owned();
        crate::types::identifier(&out_trade_no)?;
        let transaction_id = get("transaction_id")?.to_owned();
        Ok(VerifiedNotification {
            identity: format!("wechat:{}:{}:{transaction_id}", self.mch_id, self.app_id),
            out_trade_no,
            transaction_id,
            amount: Amount::fromFen(
                transaction["amount"]["total"]
                    .as_i64()
                    .ok_or(Error::Protocol)?,
            )?,
            provider: "wechat",
        })
    }
}
