use http::{HeaderMap, HeaderValue, Method};
use openrust::{
    events::Dispatcher,
    http::{HttpFuture, Response, StatusCode},
};
use openrust_webhook_client::{
    Error, RespondsToWebhook, Result, SignatureValidator, VerifiedWebhook, WebhookConfig,
    WebhookJobDispatcher, WebhookProcessor, WebhookProfile, WebhookRequest, WebhookRuntime,
    WebhookStore, models::WebhookCall,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
type Trace = Arc<Mutex<Vec<&'static str>>>;
struct Store {
    trace: Trace,
    calls: Mutex<Vec<WebhookCall>>,
    fail: bool,
    fail_clear: bool,
}
impl WebhookStore for Store {
    fn store_webhook<'a>(
        &'a self,
        webhook: &'a VerifiedWebhook<'_>,
    ) -> HttpFuture<'a, Result<WebhookCall>> {
        Box::pin(async move {
            self.trace.lock().unwrap().push("store");
            if self.fail {
                return Err(Error::InvalidRecord);
            }
            let call = WebhookCall {
                id: "fixture-record".into(),
                scope: "application-tenant".into(),
                name: webhook.config().name().into(),
                revision: webhook.config().revision().into(),
                url: webhook.request().uri().to_string(),
                method: webhook.request().method().clone(),
                headers: webhook
                    .config()
                    .headers_to_store(webhook.request().headers())?,
                raw_body: webhook.request().body().to_vec(),
                created_at: 1,
                exception: None,
            };
            self.calls.lock().unwrap().push(call.clone());
            Ok(call)
        })
    }
    fn find<'a>(&'a self, id: &'a str) -> HttpFuture<'a, Result<Option<WebhookCall>>> {
        Box::pin(async move {
            Ok(self
                .calls
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id() == id)
                .cloned())
        })
    }
    fn set_exception<'a>(
        &'a self,
        call: &'a WebhookCall,
        error: Option<&'a Value>,
    ) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            self.trace.lock().unwrap().push(if error.is_some() {
                "exception"
            } else {
                "clear"
            });
            if error.is_none() && self.fail_clear {
                return Err(Error::InvalidConfig("clear fixture"));
            }
            self.calls
                .lock()
                .unwrap()
                .iter_mut()
                .find(|c| c.id() == call.id())
                .unwrap()
                .exception = error.cloned();
            Ok(())
        })
    }
}
struct Job(Trace, bool);
impl WebhookJobDispatcher for Job {
    fn dispatch<'a>(&'a self, call: &'a WebhookCall) -> HttpFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.lock().unwrap().push("job");
            assert_eq!(call.raw_body(), b"original\0\xff");
            if self.1 {
                Err(Error::InvalidConfig("dispatch fixture"))
            } else {
                Ok(())
            }
        })
    }
}
struct Validator(Trace, Option<bool>);
impl SignatureValidator for Validator {
    fn is_valid(&self, _: &WebhookRequest, _: &WebhookConfig) -> Result<bool> {
        self.0.lock().unwrap().push("validate");
        self.1.ok_or(Error::InvalidConfig("validator fixture"))
    }
}
struct Profile(Trace, bool);
impl WebhookProfile for Profile {
    fn should_process(&self, _: &WebhookRequest, _: &WebhookConfig) -> Result<bool> {
        self.0.lock().unwrap().push("profile");
        Ok(self.1)
    }
}
struct Respond(Trace, bool);
impl RespondsToWebhook for Respond {
    fn respond_to_valid_webhook(&self, _: &WebhookRequest, _: &WebhookConfig) -> Result<Response> {
        self.0.lock().unwrap().push("response");
        if self.1 {
            Err(Error::InvalidConfig("response fixture"))
        } else {
            Ok(Response::new(StatusCode::ACCEPTED, "accepted".into()))
        }
    }
}
fn request() -> WebhookRequest {
    WebhookRequest::new(Method::POST, HeaderMap::new(), b"original\0\xff".to_vec())
        .unwrap()
        .with_uri("/original?x=%2F".parse().unwrap())
}
struct Case {
    valid: Option<bool>,
    process: bool,
    store_fails: bool,
    job_fails: bool,
    response_fails: bool,
    clear_fails: bool,
}
impl Default for Case {
    fn default() -> Self {
        Self {
            valid: Some(true),
            process: true,
            store_fails: false,
            job_fails: false,
            response_fails: false,
            clear_fails: false,
        }
    }
}
async fn run(
    case: Case,
    expected: &[&'static str],
) -> (
    Arc<Store>,
    Arc<openrust::events::EventFake>,
    Result<Response>,
) {
    let trace = Trace::default();
    let store = Arc::new(Store {
        trace: trace.clone(),
        calls: Mutex::default(),
        fail: case.store_fails,
        fail_clear: case.clear_fails,
    });
    let events = Arc::new(Dispatcher::new());
    let fake = events.fake(&[]);
    let runtime = WebhookRuntime::new(
        store.clone(),
        Arc::new(Job(trace.clone(), case.job_fails)),
        events,
    )
    .with_signature_validator(Arc::new(Validator(trace.clone(), case.valid)))
    .with_webhook_profile(Arc::new(Profile(trace.clone(), case.process)))
    .with_webhook_response(Arc::new(Respond(trace.clone(), case.response_fails)));
    let result = WebhookProcessor::new(
        &request(),
        &WebhookConfig::new("supplier", "private-secret").unwrap(),
        &runtime,
    )
    .process()
    .await;
    assert_eq!(*trace.lock().unwrap(), expected);
    (store, fake, result)
}
#[tokio::test]
async fn processor_preserves_storage_dispatch_response_order_and_original_payload() {
    let (store, _, result) = run(
        Case::default(),
        &["validate", "profile", "store", "clear", "job", "response"],
    )
    .await;
    assert_eq!(result.unwrap().status(), StatusCode::ACCEPTED);
    let call = WebhookCall::find(store.as_ref(), "fixture-record")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(call.url(), "/original?x=%2F");
    assert_eq!(call.revision(), "1");
    assert!(call.exception().is_none());
}
#[tokio::test]
async fn invalid_signature_emits_typed_event_before_error_and_never_stores() {
    let (store, fake, result) = run(
        Case {
            valid: Some(false),
            ..Default::default()
        },
        &["validate"],
    )
    .await;
    assert!(matches!(result, Err(Error::InvalidSignature)));
    assert!(store.calls.lock().unwrap().is_empty());
    let events = fake.dispatched("openrust_webhook_client::events::InvalidWebhookSignatureEvent");
    assert_eq!(events.len(), 1);
    let event = events[0]
        .downcast_ref::<openrust_webhook_client::events::InvalidWebhookSignatureEvent>()
        .unwrap();
    assert_eq!(event.request.body(), request().body());
    assert_eq!(event.config.name(), "supplier");
    assert!(events[0].properties().is_none());
}
#[tokio::test]
async fn validator_error_is_not_a_bad_signature_event_or_successful_hint() {
    let (_, fake, result) = run(
        Case {
            valid: None,
            ..Default::default()
        },
        &["validate"],
    )
    .await;
    assert!(matches!(result, Err(Error::InvalidConfig(_))));
    fake.assert_nothing_dispatched();
}
#[tokio::test]
async fn ignored_valid_event_only_responds_without_storage_or_dispatch() {
    let (store, _, result) = run(
        Case {
            process: false,
            ..Default::default()
        },
        &["validate", "profile", "response"],
    )
    .await;
    assert_eq!(result.unwrap().status(), StatusCode::ACCEPTED);
    assert!(store.calls.lock().unwrap().is_empty());
}
#[tokio::test]
async fn storage_failure_cannot_dispatch_or_acknowledge() {
    let (_, _, result) = run(
        Case {
            store_fails: true,
            ..Default::default()
        },
        &["validate", "profile", "store"],
    )
    .await;
    assert!(matches!(result, Err(Error::InvalidRecord)));
}
#[tokio::test]
async fn dispatch_failure_is_saved_redacted_and_original_failure_propagates() {
    let (store, _, result) = run(
        Case {
            job_fails: true,
            ..Default::default()
        },
        &["validate", "profile", "store", "clear", "job", "exception"],
    )
    .await;
    assert!(matches!(
        result,
        Err(Error::InvalidConfig("dispatch fixture"))
    ));
    assert_eq!(
        store.calls.lock().unwrap()[0].exception(),
        Some(&json!({"code":0,"message":"Webhook 处理任务派发失败"}))
    );
}
#[tokio::test]
async fn response_failure_does_not_erase_saved_receipt_or_retract_dispatch() {
    let (store, _, result) = run(
        Case {
            response_fails: true,
            ..Default::default()
        },
        &["validate", "profile", "store", "clear", "job", "response"],
    )
    .await;
    assert!(result.is_err());
    assert_eq!(store.calls.lock().unwrap().len(), 1);
}
#[test]
fn header_selection_preserves_duplicate_and_opaque_values_with_explicit_wildcard() {
    let mut headers = HeaderMap::new();
    headers.append("x-duplicate", "one".parse().unwrap());
    headers.append("x-duplicate", "two".parse().unwrap());
    headers.insert("x-opaque", HeaderValue::from_bytes(b"\xff").unwrap());
    headers.insert("authorization", "private".parse().unwrap());
    let config = WebhookConfig::new("supplier", "secret").unwrap();
    assert!(
        WebhookCall::headers_to_store(&config, &request())
            .unwrap()
            .is_empty()
    );
    assert!(config.headers_to_store(&headers).unwrap().is_empty());
    let selected = config
        .clone()
        .with_store_headers(&["X-Duplicate", "X-Opaque"])
        .unwrap()
        .headers_to_store(&headers)
        .unwrap();
    assert_eq!(selected.get_all("x-duplicate").iter().count(), 2);
    assert_eq!(selected["x-opaque"].as_bytes(), b"\xff");
    assert!(!selected.contains_key("authorization"));
    assert_eq!(
        config
            .clone()
            .with_store_headers(&["*"])
            .unwrap()
            .headers_to_store(&headers)
            .unwrap(),
        headers
    );
    assert!(
        config
            .clone()
            .with_store_headers(&["*", "authorization"])
            .is_err()
    );
    assert!(config.with_store_headers(&["bad\nheader"]).is_err());
}
#[tokio::test]
async fn processor_budget_applies_even_when_a_custom_validator_always_accepts() {
    let trace = Trace::default();
    let store = Arc::new(Store {
        trace: trace.clone(),
        calls: Mutex::default(),
        fail: false,
        fail_clear: false,
    });
    let runtime = WebhookRuntime::new(
        store,
        Arc::new(Job(trace.clone(), false)),
        Arc::new(Dispatcher::new()),
    )
    .with_signature_validator(Arc::new(Validator(trace.clone(), Some(true))));
    let config = WebhookConfig::new("supplier", "secret")
        .unwrap()
        .with_max_body_bytes(1)
        .unwrap();
    assert!(matches!(
        WebhookProcessor::new(&request(), &config, &runtime)
            .process()
            .await,
        Err(Error::PayloadTooLarge)
    ));
    assert!(trace.lock().unwrap().is_empty());
}

#[tokio::test]
async fn exception_clear_failure_is_recorded_without_dispatching_or_responding() {
    let (store, _, result) = run(
        Case {
            clear_fails: true,
            ..Default::default()
        },
        &["validate", "profile", "store", "clear", "exception"],
    )
    .await;
    assert!(matches!(result, Err(Error::InvalidConfig("clear fixture"))));
    assert!(store.calls.lock().unwrap()[0].exception().is_some());
}
