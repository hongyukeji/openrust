use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use aws_lc_rs::{digest, hmac};
use axum::{Router, extract::State, response::IntoResponse};
use http::{HeaderMap, StatusCode, Uri};
use openrust::{
    container::Callback,
    foundation::Application,
    notifications::{
        Notifiable, Notification, NotificationResult, NotificationRoute,
        channels::{Channel, SmsChannel},
        sms::{SmsDeliveryStatus, SmsMessage, SmsSubmissionStatus, SmsTemplate},
    },
    support::facades::{Notification as Notify, Sms},
};
use serde_json::{Value, json};

const TWILIO_PATH: &str = "/2010-04-01/Accounts/ACfixture/Messages.json";

fn sha256(value: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, value).as_ref())
}

fn hash(key: &[u8], value: &str) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), value.as_bytes())
        .as_ref()
        .to_vec()
}

fn hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn verify_aliyun(headers: &HeaderMap, query: &str, body: &str) -> Value {
    assert!(body.is_empty());
    assert_eq!(headers["x-acs-action"], "SendSms");
    assert_eq!(headers["x-acs-version"], "2017-05-25");
    assert_eq!(headers["x-acs-content-sha256"], sha256(b""));
    let authorization = headers["authorization"].to_str().unwrap();
    let (scheme, signature) = authorization.rsplit_once(",Signature=").unwrap();
    let keys = scheme
        .strip_prefix("ACS3-HMAC-SHA256 Credential=fixture-id,SignedHeaders=")
        .unwrap();
    let mut canonical = format!("POST\n/\n{query}\n");
    for key in keys.split(';') {
        canonical.push_str(&format!("{key}:{}\n", headers[key].to_str().unwrap()));
    }
    canonical.push_str(&format!("\n{keys}\n{}", sha256(b"")));
    let data = format!("ACS3-HMAC-SHA256\n{}", sha256(canonical.as_bytes()));
    assert_eq!(signature, hex(&hash(b"aliyun-secret", &data)));
    let fields: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(fields["PhoneNumbers"], "13800138000");
    assert_eq!(fields["SignName"], "示例 + 签名");
    assert_eq!(fields["TemplateParam"], r#"{"code":"A + 中文"}"#);
    if fields["TemplateCode"] == "reject" {
        return json!({"Code":"isv.TEST","Message":"fixture private response"});
    }
    assert_eq!(fields["TemplateCode"], "SMS_1");
    json!({"Code":"OK","BizId":"aliyun-id"})
}

fn verify_tencent(headers: &HeaderMap, body: &str) -> Value {
    assert_eq!(headers["x-tc-action"], "SendSms");
    assert_eq!(headers["x-tc-version"], "2021-01-11");
    assert_eq!(headers["x-tc-region"], "ap-guangzhou");
    let timestamp = headers["x-tc-timestamp"].to_str().unwrap();
    let time = chrono::DateTime::from_timestamp(timestamp.parse().unwrap(), 0).unwrap();
    let date = time.format("%Y-%m-%d").to_string();
    let scope = format!("{date}/sms/tc3_request");
    let host = headers["host"].to_str().unwrap();
    let content_type = headers["content-type"].to_str().unwrap();
    let canonical = format!(
        "POST\n/\n\ncontent-type:{content_type}\nhost:{host}\n\ncontent-type;host\n{}",
        sha256(body.as_bytes())
    );
    let data = format!(
        "TC3-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        sha256(canonical.as_bytes())
    );
    let day = hash(b"TC3tencent-secret", &date);
    let service = hash(&day, "sms");
    let signing = hash(&service, "tc3_request");
    assert_eq!(
        headers["authorization"].to_str().unwrap(),
        format!(
            "TC3-HMAC-SHA256 Credential=fixture-id/{scope}, SignedHeaders=content-type;host, Signature={}",
            hex(&hash(&signing, &data))
        )
    );
    let json: Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["PhoneNumberSet"], json!(["+85255550123"]));
    assert_eq!(json["SignName"], "测试签名");
    assert_eq!(json["SmsSdkAppId"], "fixture-app");
    assert_eq!(json["TemplateParamSet"], json!(["42", "5"]));
    if json["TemplateId"] == "reject" {
        return json!({"Response":{"SendStatusSet":[{"Code":"Failed","SerialNo":""}]}});
    }
    assert_eq!(json["TemplateId"], "1234");
    json!({"Response":{"SendStatusSet":[{"Code":"Ok","SerialNo":"tencent-id"}]}})
}

async fn receive(
    State(count): State<Arc<AtomicUsize>>,
    uri: Uri,
    headers: HeaderMap,
    body: String,
) -> axum::response::Response {
    count.fetch_add(1, Ordering::Relaxed);
    let payload = if uri.path() == "/vonage" {
        let fields: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect();
        assert_eq!(fields["to"], "12025550123");
        match fields["text"].as_str() {
            "Price €5" => assert_eq!(fields["type"], "text"),
            "短信演示" => assert_eq!(fields["type"], "unicode"),
            text => panic!("unexpected SMS fixture text: {text}"),
        }
        json!({"messages":[{"status":"0", "message-id":"vonage-id"}]})
    } else if uri.path() == TWILIO_PATH {
        assert_eq!(
            headers["authorization"],
            "Basic U0tmaXh0dXJlOnR3aWxpby1zZWNyZXQ="
        );
        let fields: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect();
        assert_eq!(fields["To"], "+12025550123");
        assert_eq!(fields["MessagingServiceSid"], "MGfixture");
        if let Some(content) = fields.get("Body") {
            assert_eq!(content, "Hello 世界");
            assert!(!fields.contains_key("ContentSid"));
        } else {
            assert_eq!(fields["ContentSid"], "HXfixture");
            assert_eq!(fields["ContentVariables"], r#"{"1":"42"}"#);
        }
        return (
            StatusCode::CREATED,
            [("content-type", "application/json")],
            r#"{"sid":"SMfixture","status":"queued"}"#,
        )
            .into_response();
    } else if headers.contains_key("x-acs-action") {
        verify_aliyun(&headers, uri.query().unwrap(), &body)
    } else {
        verify_tencent(&headers, &body)
    };
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        payload.to_string(),
    )
        .into_response()
}

struct CodeNotice;

impl Notification for CodeNotice {
    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["sms".into()])
    }

    fn to_sms(&self, _: &dyn Notifiable, to: String) -> NotificationResult<Option<SmsMessage>> {
        SmsMessage::template(
            to,
            SmsTemplate::named("SMS_1", [("code".into(), "A + 中文".into())].into()),
        )
        .map(Some)
    }
}

#[tokio::test]
async fn sms_builtin_drivers_use_the_provider_protocol_and_market() {
    let requests = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/", axum::routing::post(receive))
        .route(TWILIO_PATH, axum::routing::post(receive))
        .route("/vonage", axum::routing::post(receive))
        .with_state(requests.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let config = json!({"app":{"env":"testing"},
        "sms":{"default":"china","drivers":{
        "vonage":{"driver":"vonage","api_key":"fixture-key","api_secret":"fixture-secret","from":"Example","endpoint":format!("{endpoint}/vonage")},
        "china":{"driver":"aliyun","market":"mainland","endpoint":endpoint,"access_key_id":"fixture-id",
            "access_key_secret":"aliyun-secret","sign_name":"示例 + 签名"},
        "hk":{"driver":"tencent","market":"international","endpoint":endpoint,"secret_id":"fixture-id",
            "secret_key":"tencent-secret","sdk_app_id":"fixture-app","region":"ap-guangzhou","sign_name":"测试签名",
            "templates":{"login_code":"unused-default"},
            "markets":{"international":{"templates":{"login_code":{"id":"1234","parameter_order":["code","minutes"]}}}}},
        "global":{"driver":"twilio","market":"international","endpoint":endpoint,"account_sid":"ACfixture",
            "api_key_sid":"SKfixture","api_key_secret":"twilio-secret","messaging_service_sid":"MGfixture"}
    }}});
    std::fs::write(root.path().join("config.json"), config.to_string()).unwrap();
    let mut app = Application::configure(root.path().into())
        .withConfig(|base, _| {
            Ok(serde_json::from_slice(&std::fs::read(base.join("config.json")).unwrap()).unwrap())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    app.call(
        Callback::new(&[], |_| async {
            let recipient =
                Notify::route("sms", NotificationRoute::Sms("13800138000".into())).unwrap();
            Notify::sendNow(&[&recipient], &CodeNotice, None)
                .await
                .unwrap();

            let channel = SmsChannel::default();
            let queued = channel
                .queue_payload(&recipient, &CodeNotice)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(queued["profile"], "china");
            assert_eq!(queued["message"]["template"]["id"], "SMS_1");
            assert!(!queued.to_string().contains("aliyun-secret"));
            channel.send_queued(queued).await.unwrap();

            let alibaba = Sms::default_driver().unwrap();
            let template =
                || SmsTemplate::named("SMS_1", [("code".into(), "A + 中文".into())].into());
            let local = SmsMessage::template("+8613800138000", template()).unwrap();
            assert_eq!(alibaba.send(local).await.unwrap()["BizId"], "aliyun-id");
            assert!(
                alibaba
                    .send(SmsMessage::template("+85255550123", template()).unwrap())
                    .await
                    .is_err()
            );
            assert!(
                alibaba
                    .send(SmsMessage::new("13800138000", "text").unwrap())
                    .await
                    .is_err()
            );
            let mut rejected = template();
            rejected.id = "reject".into();
            let error = alibaba
                .send(SmsMessage::template("13800138000", rejected).unwrap())
                .await
                .unwrap_err();
            assert!(!error.to_string().contains("fixture private response"));

            let tencent = Sms::driver("hk").unwrap();
            let positional = |id| SmsTemplate::positional(id, vec!["42".into(), "5".into()]);
            let result = tencent
                .send(SmsMessage::template("+85255550123", positional("1234")).unwrap())
                .await
                .unwrap();
            assert_eq!(
                result["Response"]["SendStatusSet"][0]["SerialNo"],
                "tencent-id"
            );
            assert!(
                tencent
                    .send(SmsMessage::template("+85255550123", positional("reject")).unwrap())
                    .await
                    .is_err()
            );
            assert!(
                tencent
                    .send(SmsMessage::template("+85255550123", template()).unwrap())
                    .await
                    .is_err()
            );
            assert!(
                tencent
                    .send(SmsMessage::template("+8613800138000", positional("1234")).unwrap())
                    .await
                    .is_err()
            );

            let mut configured = tencent
                .send_template(
                    "+85255550123",
                    "login_code",
                    [("minutes", "5"), ("code", "42")],
                )
                .await
                .unwrap();
            assert_eq!(configured.status, SmsSubmissionStatus::Accepted);
            assert_eq!(configured.message_ids, ["tencent-id"]);
            assert!(!configured.record_delivery(
                "twilio",
                "tencent-id",
                SmsDeliveryStatus::Delivered
            ));
            assert!(configured.record_delivery(
                "tencent",
                "tencent-id",
                SmsDeliveryStatus::Delivered
            ));
            assert_eq!(configured.status, SmsSubmissionStatus::Delivered);
            assert!(!configured.record_delivery(
                "tencent",
                "tencent-id",
                SmsDeliveryStatus::Undelivered
            ));
            assert!(
                tencent
                    .send_template("+85255550123", "login_code", [("code", "42")])
                    .await
                    .is_err()
            );

            let twilio = Sms::driver("global").unwrap();
            let text = SmsMessage::new("+12025550123", "Hello 世界").unwrap();
            assert_eq!(
                serde_json::to_value(&text).unwrap(),
                json!({"to":"+12025550123","content":"Hello 世界"})
            );
            assert_eq!(twilio.send(text).await.unwrap()["status"], "queued");
            let template = SmsTemplate::named("HXfixture", [("1".into(), "42".into())].into());
            assert_eq!(
                twilio
                    .send(SmsMessage::template("+12025550123", template).unwrap())
                    .await
                    .unwrap()["sid"],
                "SMfixture"
            );
            assert!(
                twilio
                    .send(SmsMessage::new("12025550123", "text").unwrap())
                    .await
                    .is_err()
            );
            let vonage = Sms::driver("vonage").unwrap();
            for text in ["Price €5", "短信演示"] {
                let sent = vonage.send_text("+12025550123", text).await.unwrap();
                assert_eq!(sent.message_ids, ["vonage-id"]);
                assert_eq!(sent.status, SmsSubmissionStatus::Accepted);
            }
            Ok(())
        }),
        Default::default(),
    )
    .await
    .unwrap();
    assert_eq!(requests.load(Ordering::Relaxed), 11);
    server.abort();
}
