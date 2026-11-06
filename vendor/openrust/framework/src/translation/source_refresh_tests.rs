use super::*;
use crate::support::BoxFuture;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Source {
    snapshot: RwLock<Arc<dyn TranslationLoadSession>>,
    calls: AtomicUsize,
    checks: AtomicUsize,
    fail: AtomicBool,
    block: AtomicBool,
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
}
impl Source {
    fn new(session: Arc<dyn TranslationLoadSession>) -> Self {
        Self {
            snapshot: RwLock::new(session),
            calls: AtomicUsize::new(0),
            checks: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
            block: AtomicBool::new(false),
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
        }
    }
}
impl TranslationLoader for Source {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.block.load(Ordering::SeqCst) {
                self.entered.add_permits(1);
                self.release
                    .acquire()
                    .await
                    .map_err(|_| catalog_error())?
                    .forget();
            }
            if self.fail.load(Ordering::SeqCst) {
                return Err(source_error("private_source_failure"));
            }
            let snapshot = self.snapshot.read().map_err(|_| catalog_error())?;
            Ok(Arc::clone(&snapshot))
        })
    }
    fn needs_refresh(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            self.checks.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    }
}
fn session(
    entries: &[(&str, &str, Option<&str>, &str)],
) -> Result<Arc<dyn TranslationLoadSession>> {
    let mut resources = BTreeMap::new();
    for (locale, group, namespace, json) in entries {
        resources.insert(
            TranslationResource {
                locale: Locale::new(locale)?,
                group: (*group).into(),
                namespace: namespace.map(str::to_owned),
            },
            parse_translation_lines(json.as_bytes())?,
        );
    }
    translation_session(resources)
}
fn text(service: &Translator, locale: &str, key: &str) -> Result<String> {
    Ok(service.translate(&Locale::new(locale)?, key, &MessageArgs::new()))
}

// 平铺目录会把合法JSON literal与group/ns同名资源互相覆盖；本例同时保留它们。
#[tokio::test]
async fn literal_json_keys_precede_group_and_namespace_without_rejecting_collisions() -> Result<()>
{
    let source = Arc::new(Source::new(session(&[
        (
            "en",
            "*",
            None,
            r#"{"group.item":"literal","ns::group.item":"literal ns","empty":""}"#,
        ),
        (
            "en",
            "group",
            None,
            r#"{"item":"group","only":"group only"}"#,
        ),
        (
            "en",
            "group",
            Some("ns"),
            r#"{"item":"namespace","only":"namespace only"}"#,
        ),
        ("fr", "*", None, r#"{"exact":"français"}"#),
    ])?));
    let service = Translator::from_loader(TranslationConfig::default(), source).await?;
    assert_eq!(text(&service, "en", "group.item")?, "literal");
    assert_eq!(text(&service, "en", "ns::group.item")?, "literal ns");
    assert_eq!(text(&service, "en", "group.only")?, "group only");
    assert_eq!(text(&service, "en", "ns::group.only")?, "namespace only");
    assert_eq!(text(&service, "en", "empty")?, "empty");
    assert_eq!(
        text(
            &service.clone().with_lookup(TranslationLookup::Catalog),
            "en",
            "empty"
        )?,
        ""
    );
    assert_eq!(text(&service, "fr", "exact")?, "français");
    Ok(())
}

#[tokio::test]
async fn failure_preserves_catalog_cools_checks_and_explicit_retry_recovers() -> Result<()> {
    let source = Arc::new(Source::new(session(&[(
        "en",
        "*",
        None,
        r#"{"key":"old"}"#,
    )])?));
    let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    source.fail.store(true, Ordering::SeqCst);
    assert!(service.refresh_if_due().await.is_err());
    assert_eq!(text(&service, "en", "key")?, "old");
    let calls = source.calls.load(Ordering::SeqCst);
    let checks = source.checks.load(Ordering::SeqCst);
    assert!(service.refresh_if_due().await.is_err());
    assert_eq!(source.calls.load(Ordering::SeqCst), calls);
    assert_eq!(source.checks.load(Ordering::SeqCst), checks);
    assert_eq!(
        service.refresh_status()?.last_error_code,
        Some("i18n_source_refresh_failed")
    );
    source.fail.store(false, Ordering::SeqCst);
    *source.snapshot.write().map_err(|_| catalog_error())? =
        session(&[("en", "*", None, r#"{"key":"new"}"#)])?;
    service.refresh().await?;
    assert_eq!(text(&service, "en", "key")?, "new");
    assert_eq!(service.refresh_status()?.last_error_code, None);
    Ok(())
}

// 丢弃正在等待来源的future必须解除singleflight；namespace同步登记不能被旧capture覆盖。
#[tokio::test]
async fn cancelled_capture_releases_flight_and_latest_namespace_overlay_survives() -> Result<()> {
    let source = Arc::new(Source::new(session(&[(
        "en",
        "*",
        None,
        r#"{"key":"old"}"#,
    )])?));
    let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    source.block.store(true, Ordering::SeqCst);
    let mut refresh = Box::pin(service.refresh());
    tokio::select! { result = &mut refresh => { result?; return Err(catalog_error()); }, permit = source.entered.acquire() => { permit.map_err(|_| catalog_error())?.forget(); } }
    assert_eq!(text(&service, "en", "key")?, "old");
    let mut overlay = empty_catalog();
    overlay.merge_json("en", r#"{"module::key":"module"}"#)?;
    service.merge_namespace("module", &overlay)?;
    drop(refresh);
    assert!(!service.refresh_status()?.refreshing);
    assert_eq!(
        service.refresh_status()?.last_error_code,
        Some("i18n_source_refresh_cancelled")
    );
    assert!(service.refresh_if_due().await.is_err());
    source.block.store(false, Ordering::SeqCst);
    service.refresh().await?;
    assert_eq!(text(&service, "en", "module::key")?, "module");
    Ok(())
}

#[tokio::test]
async fn simultaneous_refreshes_capture_once_and_publish_complete_snapshot() -> Result<()> {
    let source = Arc::new(Source::new(session(&[(
        "en",
        "*",
        None,
        r#"{"a":"old","b":"old"}"#,
    )])?));
    let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    *source.snapshot.write().map_err(|_| catalog_error())? =
        session(&[("en", "*", None, r#"{"a":"new","b":"new"}"#)])?;
    source.block.store(true, Ordering::SeqCst);
    let first = service.refresh();
    let second = async {
        source
            .entered
            .acquire()
            .await
            .map_err(|_| catalog_error())?
            .forget();
        let second = service.refresh();
        let release = async {
            tokio::task::yield_now().await;
            source.release.add_permits(1);
        };
        let (outcome, ()) = tokio::join!(second, release);
        outcome
    };
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first?, second?);
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    assert_eq!(text(&service, "en", "a")?, "new");
    assert_eq!(text(&service, "en", "b")?, "new");
    Ok(())
}

struct InvalidSession {
    resources: Vec<TranslationResource>,
}
impl TranslationLoadSession for InvalidSession {
    fn resources(&self) -> &[TranslationResource] {
        &self.resources
    }
    fn load(&self, _: &Locale, _: &str, _: Option<&str>) -> Result<TranslationLines> {
        Ok(BTreeMap::from([("bad".into(), serde_json::json!(42))]))
    }
}
#[tokio::test]
async fn untrusted_session_validation_prevents_partial_publish_and_initial_success() -> Result<()> {
    let invalid: Arc<dyn TranslationLoadSession> = Arc::new(InvalidSession {
        resources: vec![TranslationResource {
            locale: Locale::new("en")?,
            group: "*".into(),
            namespace: None,
        }],
    });
    let source = Arc::new(Source::new(session(&[(
        "en",
        "*",
        None,
        r#"{"key":"verified"}"#,
    )])?));
    let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    let generation = service.refresh_status()?.generation;
    *source.snapshot.write().map_err(|_| catalog_error())? = invalid;
    assert!(service.refresh().await.is_err());
    assert_eq!(service.refresh_status()?.generation, generation);
    assert_eq!(
        service.refresh_status()?.last_error_code,
        Some("i18n_source_catalog_invalid")
    );
    assert_eq!(text(&service, "en", "key")?, "verified");
    assert!(
        Translator::from_loader(TranslationConfig::default(), source)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn composite_merges_exact_locale_before_fallback_and_uses_existing_formatter() -> Result<()> {
    let files = Arc::new(Source::new(session(&[(
        "fr",
        "*",
        None,
        r#"{"hello":"bonjour","empty":"file"}"#,
    )])?));
    let database_like = Arc::new(Source::new(session(&[
        ("en", "*", None, r#"{"hello":"database english"}"#),
        (
            "fr",
            "*",
            None,
            r#"{"empty":"","count":{"plural":{"argument":"n","forms":{"one":"one","other":"many"}}}}"#,
        ),
    ])?));
    let composite = Arc::new(CompositeTranslationLoader::new(vec![files, database_like]));
    let service = Translator::from_loader(TranslationConfig::default(), composite).await?;
    assert_eq!(text(&service, "fr", "hello")?, "bonjour");
    assert_eq!(text(&service, "fr", "empty")?, "empty");
    assert_eq!(
        text(
            &service.clone().with_lookup(TranslationLookup::Catalog),
            "fr",
            "empty"
        )?,
        ""
    );
    assert_eq!(
        service.translate(
            &Locale::new("fr")?,
            "count",
            &BTreeMap::from([("n".into(), "2".into())])
        ),
        "many"
    );
    Ok(())
}

#[tokio::test]
async fn namespace_registered_during_successful_capture_is_not_overwritten() -> Result<()> {
    let source = Arc::new(Source::new(session(&[(
        "en",
        "*",
        None,
        r#"{"key":"old"}"#,
    )])?));
    let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    source.block.store(true, Ordering::SeqCst);
    let refresh = service.refresh();
    let registration = async {
        source
            .entered
            .acquire()
            .await
            .map_err(|_| catalog_error())?
            .forget();
        let mut overlay = empty_catalog();
        overlay.merge_json("en", r#"{"registered":"preserved"}"#)?;
        service.merge_namespace("module", &overlay)?;
        source.release.add_permits(1);
        Ok::<(), Error>(())
    };
    let (outcome, registered) = tokio::join!(refresh, registration);
    outcome?;
    registered?;
    assert_eq!(text(&service, "en", "registered")?, "preserved");
    assert!(service.merge_namespace("module", &empty_catalog()).is_err());
    Ok(())
}

#[tokio::test]
async fn laravel_json_precedes_only_requested_locale_then_group_falls_back_with_falsey_values()
-> Result<()> {
    let source = Arc::new(Source::new(session(&[
        (
            "fr",
            "*",
            None,
            r#"{"ui.zero":"0","ui.empty":"","ns::ui.zero":"0"}"#,
        ),
        (
            "en",
            "*",
            None,
            r#"{"ui.save":"JSON EN","ns::ui.save":"JSON NS EN","only_json":"fallback JSON"}"#,
        ),
        (
            "en",
            "ui",
            None,
            r#"{"save":"GROUP EN","empty":"","zero":"0"}"#,
        ),
        (
            "en",
            "ui",
            Some("ns"),
            r#"{"save":"GROUP NS EN","empty":"","zero":"0"}"#,
        ),
    ])?));
    let service = Translator::from_loader(TranslationConfig::default(), source).await?;
    assert_eq!(text(&service, "fr", "ui.save")?, "GROUP EN");
    assert_eq!(text(&service, "fr", "ns::ui.save")?, "GROUP NS EN");
    assert_eq!(text(&service, "fr", "ui.zero")?, "ui.zero");
    assert_eq!(text(&service, "fr", "ui.empty")?, "ui.empty");
    assert_eq!(text(&service, "fr", "ns::ui.zero")?, "ns::ui.zero");
    assert_eq!(text(&service, "en", "ui.zero")?, "0");
    assert_eq!(text(&service, "en", "ui.empty")?, "");
    assert_eq!(text(&service, "fr", "ns::ui.empty")?, "");
    assert_eq!(text(&service, "fr", "only_json")?, "only_json");
    let enhanced = service.with_lookup(TranslationLookup::Catalog);
    assert_eq!(text(&enhanced, "fr", "ui.save")?, "JSON EN");
    assert_eq!(text(&enhanced, "fr", "ui.zero")?, "0");
    assert_eq!(text(&enhanced, "fr", "ui.empty")?, "");
    assert_eq!(text(&enhanced, "fr", "only_json")?, "fallback JSON");
    Ok(())
}

struct AdversarialSession {
    resources: Vec<TranslationResource>,
    lines: TranslationLines,
}
impl TranslationLoadSession for AdversarialSession {
    fn resources(&self) -> &[TranslationResource] {
        &self.resources
    }
    fn load(&self, _: &Locale, _: &str, _: Option<&str>) -> Result<TranslationLines> {
        Ok(self.lines.clone())
    }
}
#[tokio::test]
async fn composite_cannot_wash_invalid_values_or_duplicate_resources_by_later_override()
-> Result<()> {
    let original = session(&[("en", "*", None, r#"{"key":"old"}"#)])?;
    let lower = Arc::new(Source::new(original));
    let higher = Arc::new(Source::new(session(&[(
        "en",
        "*",
        None,
        r#"{"key":"old"}"#,
    )])?));
    let service = Translator::from_loader(
        TranslationConfig::default(),
        Arc::new(CompositeTranslationLoader::new(vec![
            lower.clone(),
            higher.clone(),
        ])),
    )
    .await?;
    *higher.snapshot.write().map_err(|_| catalog_error())? =
        session(&[("en", "*", None, r#"{"key":"valid override"}"#)])?;
    let resource = TranslationResource {
        locale: Locale::new("en")?,
        group: "*".into(),
        namespace: None,
    };
    let generation = service.refresh_status()?.generation;
    for (resources, value) in [
        (vec![resource.clone()], serde_json::json!(42)),
        (
            vec![resource.clone(), resource.clone()],
            serde_json::json!("otherwise valid"),
        ),
    ] {
        *lower.snapshot.write().map_err(|_| catalog_error())? = Arc::new(AdversarialSession {
            resources,
            lines: BTreeMap::from([("key".into(), value)]),
        });
        assert!(service.refresh().await.is_err());
        assert_eq!(service.refresh_status()?.generation, generation);
        assert_eq!(text(&service, "en", "key")?, "old");
    }
    Ok(())
}

struct ReentrantSource {
    session: Arc<dyn TranslationLoadSession>,
    service: std::sync::Mutex<Option<std::sync::Weak<Translator>>>,
    from_check: AtomicBool,
    seen: AtomicUsize,
}
impl ReentrantSource {
    async fn reenter(&self) -> Result<()> {
        let service = self
            .service
            .lock()
            .map_err(|_| catalog_error())?
            .as_ref()
            .and_then(std::sync::Weak::upgrade);
        if let Some(service) = service {
            assert!(
                matches!(service.refresh().await,Err(error) if error.code()=="i18n_source_refresh_reentrant")
            );
            self.seen.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
}
impl TranslationLoader for ReentrantSource {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async move {
            if !self.from_check.load(Ordering::SeqCst) {
                self.reenter().await?;
            }
            Ok(Arc::clone(&self.session))
        })
    }
    fn needs_refresh(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            if self.from_check.load(Ordering::SeqCst) {
                self.reenter().await?;
            }
            Ok(true)
        })
    }
}
#[tokio::test]
async fn loader_direct_reentry_during_capture_and_check_returns_immediately() -> Result<()> {
    let source = Arc::new(ReentrantSource {
        session: session(&[("en", "*", None, r#"{"key":"value"}"#)])?,
        service: std::sync::Mutex::new(None),
        from_check: AtomicBool::new(false),
        seen: AtomicUsize::new(0),
    });
    let service =
        Arc::new(Translator::from_loader(TranslationConfig::default(), source.clone()).await?);
    *source.service.lock().map_err(|_| catalog_error())? = Some(Arc::downgrade(&service));
    tokio::time::timeout(Duration::from_secs(1), service.refresh())
        .await
        .map_err(|_| catalog_error())??;
    source.from_check.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(1), service.refresh_if_due())
        .await
        .map_err(|_| catalog_error())??;
    assert_eq!(source.seen.load(Ordering::SeqCst), 2);
    assert!(!service.refresh_status()?.refreshing);
    Ok(())
}

struct RevisionSession {
    inner: Arc<dyn TranslationLoadSession>,
    revision: usize,
}
impl TranslationLoadSession for RevisionSession {
    fn resources(&self) -> &[TranslationResource] {
        self.inner.resources()
    }
    fn load(
        &self,
        locale: &Locale,
        group: &str,
        namespace: Option<&str>,
    ) -> Result<TranslationLines> {
        self.inner.load(locale, group, namespace)
    }
}
struct RevisionSource {
    current: AtomicUsize,
    captured: AtomicUsize,
}
impl TranslationLoader for RevisionSource {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async move {
            let revision = self.current.load(Ordering::SeqCst);
            let json = serde_json::to_string(&BTreeMap::from([("value", revision.to_string())]))
                .map_err(|_| catalog_error())?;
            let inner = session(&[("en", "*", None, &json)])?;
            self.captured.store(revision, Ordering::SeqCst);
            Ok(Arc::new(RevisionSession { inner, revision }) as Arc<dyn TranslationLoadSession>)
        })
    }
    fn needs_refresh(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            Ok(self.current.load(Ordering::SeqCst) > self.captured.load(Ordering::SeqCst))
        })
    }
    fn needs_refresh_since<'a>(
        &'a self,
        published: &'a dyn TranslationLoadSession,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let prior = (published as &dyn std::any::Any)
                .downcast_ref::<RevisionSession>()
                .ok_or_else(catalog_error)?;
            Ok(self.current.load(Ordering::SeqCst) > prior.revision)
        })
    }
}
#[tokio::test]
async fn independent_consumers_and_public_capture_cannot_consume_another_published_cursor()
-> Result<()> {
    for composite in [false, true] {
        let source = Arc::new(RevisionSource {
            current: AtomicUsize::new(1),
            captured: AtomicUsize::new(0),
        });
        let loader: Arc<dyn TranslationLoader> = if composite {
            Arc::new(CompositeTranslationLoader::new(vec![source.clone()]))
        } else {
            source.clone()
        };
        let first = Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
        let second = Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
        source.current.store(2, Ordering::SeqCst);
        first.refresh().await?;
        second.refresh_if_due().await?;
        assert_eq!(text(&first, "en", "value")?, "2");
        assert_eq!(text(&second, "en", "value")?, "2");
        source.current.store(3, Ordering::SeqCst);
        loader.begin_snapshot().await?;
        first.refresh_if_due().await?;
        second.refresh_if_due().await?;
        assert_eq!(text(&first, "en", "value")?, "3");
        assert_eq!(text(&second, "en", "value")?, "3");
    }
    Ok(())
}

#[test]
fn public_parser_is_syntax_only_and_session_performs_message_validation() -> Result<()> {
    let lines = parse_translation_lines(br#"{"x":42}"#)?;
    let resource = TranslationResource {
        locale: Locale::new("en")?,
        group: "*".into(),
        namespace: None,
    };
    assert!(translation_session(BTreeMap::from([(resource, lines)])).is_err());
    assert!(parse_translation_lines(br#"{"x":{"other":"a","other":"b"}}"#).is_err());
    Ok(())
}

#[tokio::test]
async fn cancelled_owner_notifies_existing_waiter_and_next_explicit_attempt_recovers() -> Result<()>
{
    let source = Arc::new(Source::new(session(&[(
        "en",
        "*",
        None,
        r#"{"key":"old"}"#,
    )])?));
    let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    source.block.store(true, Ordering::SeqCst);
    let mut first = Box::pin(service.refresh());
    tokio::select! { outcome=&mut first=>{outcome?;return Err(catalog_error());}, permit=source.entered.acquire()=>{permit.map_err(|_|catalog_error())?.forget();} }
    let mut waiter = Box::pin(service.refresh());
    std::future::poll_fn(|context| {
        assert!(waiter.as_mut().poll(context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(first);
    let outcome = tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .map_err(|_| catalog_error())?;
    assert!(matches!(outcome,Err(error) if error.code()=="i18n_source_refresh_cancelled"));
    assert_eq!(text(&service, "en", "key")?, "old");
    source.block.store(false, Ordering::SeqCst);
    service.refresh().await?;
    assert_eq!(service.refresh_status()?.last_error_code, None);
    Ok(())
}

struct CheckOnlySource {
    inner: Source,
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
}
impl TranslationLoader for CheckOnlySource {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        self.inner.begin_snapshot()
    }
    fn needs_refresh_since<'a>(
        &'a self,
        _: &'a dyn TranslationLoadSession,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            self.entered.add_permits(1);
            self.release
                .acquire()
                .await
                .map_err(|_| catalog_error())?
                .forget();
            Ok(false)
        })
    }
}
#[tokio::test]
async fn explicit_refresh_waiting_on_false_auto_check_performs_one_shared_capture() -> Result<()> {
    let source = Arc::new(CheckOnlySource {
        inner: Source::new(session(&[("en", "*", None, r#"{"key":"old"}"#)])?),
        entered: tokio::sync::Semaphore::new(0),
        release: tokio::sync::Semaphore::new(0),
    });
    let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    let mut automatic = Box::pin(service.refresh_if_due());
    tokio::select! { result=&mut automatic=>{result?;return Err(catalog_error());},permit=source.entered.acquire()=>{permit.map_err(|_|catalog_error())?.forget();} }
    *source.inner.snapshot.write().map_err(|_| catalog_error())? =
        session(&[("en", "*", None, r#"{"key":"new"}"#)])?;
    source.inner.block.store(true, Ordering::SeqCst);
    let mut first = Box::pin(service.refresh());
    let mut second = Box::pin(service.refresh());
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(first.as_mut(), cx).is_pending());
        assert!(std::future::Future::poll(second.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    source.release.add_permits(1);
    let release = async {
        source
            .inner
            .entered
            .acquire()
            .await
            .map_err(|_| catalog_error())?
            .forget();
        source.inner.release.add_permits(1);
        Ok::<(), Error>(())
    };
    let (automatic, first, second, released) = tokio::join!(automatic, first, second, release);
    assert!(matches!(
        automatic?,
        TranslationRefreshOutcome::Unchanged { .. }
    ));
    assert_eq!(first?, second?);
    released?;
    assert_eq!(source.inner.calls.load(Ordering::SeqCst), 2);
    assert_eq!(text(&service, "en", "key")?, "new");
    Ok(())
}
#[tokio::test]
async fn explicit_unsupported_locale_checks_its_json_then_only_configured_fallback_group()
-> Result<()> {
    let source = Arc::new(Source::new(session(&[
        ("en", "*", None, r#"{"ui.save":"JSON","false {name}":"0"}"#),
        ("en", "ui", None, r#"{"save":"EN GROUP"}"#),
        ("zh-CN", "ui", None, r#"{"save":"FALLBACK GROUP"}"#),
    ])?));
    let service = Translator::from_loader(
        TranslationConfig {
            default_locale: Locale::new("en")?,
            fallback_locale: Locale::new("zh-CN")?,
        },
        source,
    )
    .await?;
    assert_eq!(text(&service, "fr", "ui.save")?, "FALLBACK GROUP");
    assert_eq!(text(&service, "en-GB", "ui.save")?, "FALLBACK GROUP");
    assert_eq!(
        service.translate(
            &Locale::new("en")?,
            "false {name}",
            &BTreeMap::from([("name".into(), "Ada".into())])
        ),
        "false Ada"
    );
    assert_eq!(
        text(
            &service.clone().with_lookup(TranslationLookup::Catalog),
            "en-GB",
            "ui.save"
        )?,
        "JSON"
    );
    Ok(())
}

struct PanicSession {
    inner: Arc<dyn TranslationLoadSession>,
}
impl TranslationLoadSession for PanicSession {
    fn resources(&self) -> &[TranslationResource] {
        self.inner.resources()
    }
    fn load(
        &self,
        locale: &Locale,
        group: &str,
        namespace: Option<&str>,
    ) -> Result<TranslationLines> {
        self.inner.load(locale, group, namespace)
    }
}
impl Drop for PanicSession {
    #[allow(
        clippy::panic,
        reason = "故意模拟第三方session析构panic，核已提交目录不被取消收据改写"
    )]
    fn drop(&mut self) {
        panic!("intentional plugin session destructor failure");
    }
}
struct PanicSource {
    calls: AtomicUsize,
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
}
impl TranslationLoader for PanicSource {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                return Ok(Arc::new(PanicSession {
                    inner: session(&[("en", "*", None, r#"{"key":"old"}"#)])?,
                }) as Arc<dyn TranslationLoadSession>);
            }
            self.entered.add_permits(1);
            self.release
                .acquire()
                .await
                .map_err(|_| catalog_error())?
                .forget();
            session(&[("en", "*", None, r#"{"key":"new"}"#)])
        })
    }
    fn needs_refresh(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}
#[tokio::test]
async fn previous_session_drop_panic_propagates_owner_after_success_commit_and_waiter_notification()
-> Result<()> {
    use futures_util::FutureExt as _;
    let source = Arc::new(PanicSource {
        calls: AtomicUsize::new(0),
        entered: tokio::sync::Semaphore::new(0),
        release: tokio::sync::Semaphore::new(0),
    });
    let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    let before = service.refresh_status()?.generation;
    let mut owner = Box::pin(std::panic::AssertUnwindSafe(service.refresh_if_due()).catch_unwind());
    tokio::select! { _=&mut owner=>{return Err(catalog_error());},permit=source.entered.acquire()=>{permit.map_err(|_|catalog_error())?.forget();} }
    let mut waiter = Box::pin(service.refresh());
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(waiter.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    source.release.add_permits(1);
    let (owner, waiter) = tokio::join!(owner, waiter);
    assert!(owner.is_err());
    assert_eq!(
        waiter?,
        TranslationRefreshOutcome::Updated {
            generation: before + 1
        }
    );
    assert_eq!(text(&service, "en", "key")?, "new");
    let status = service.refresh_status()?;
    assert_eq!(status.generation, before + 1);
    assert!(!status.refreshing);
    assert_eq!(status.last_error_code, None);
    Ok(())
}

#[tokio::test]
async fn delayed_explicit_waiter_reuses_immediate_successor_capture_even_after_slot_is_cleared()
-> Result<()> {
    for fail_capture in [false, true] {
        let source = Arc::new(CheckOnlySource {
            inner: Source::new(session(&[("en", "*", None, r#"{"key":"old"}"#)])?),
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
        });
        let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
        let mut automatic = Box::pin(service.refresh_if_due());
        tokio::select! { result=&mut automatic=>{result?;return Err(catalog_error());},permit=source.entered.acquire()=>{permit.map_err(|_|catalog_error())?.forget();} }
        let mut first = Box::pin(service.refresh());
        let mut delayed = Box::pin(service.refresh());
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(first.as_mut(), cx).is_pending());
            assert!(std::future::Future::poll(delayed.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        *source.inner.snapshot.write().map_err(|_| catalog_error())? =
            session(&[("en", "*", None, r#"{"key":"new"}"#)])?;
        source.inner.fail.store(fail_capture, Ordering::SeqCst);
        // 只挂起自动检查。capture不设barrier，E1完整返回后才重新poll E2。
        source.release.add_permits(1);
        assert!(matches!(
            automatic.await?,
            TranslationRefreshOutcome::Unchanged { .. }
        ));
        let first = first.await;
        source.inner.fail.store(false, Ordering::SeqCst);
        let delayed = delayed.await;
        match (first, delayed) {
            (Ok(first), Ok(delayed)) => {
                assert!(!fail_capture);
                assert_eq!(first, delayed);
                assert_eq!(text(&service, "en", "key")?, "new");
            }
            (Err(first), Err(delayed)) => {
                assert!(fail_capture);
                assert_eq!(first.code(), delayed.code());
                assert_eq!(text(&service, "en", "key")?, "old");
            }
            _ => return Err(catalog_error()),
        }
        assert_eq!(source.inner.calls.load(Ordering::SeqCst), 2);
        // 后来的独立显式请求仍可重新捕获/重试，不能被旧检查的后继缓存永久吞掉。
        service.refresh().await?;
        assert_eq!(source.inner.calls.load(Ordering::SeqCst), 3);
        assert_eq!(text(&service, "en", "key")?, "new");
    }
    Ok(())
}

// 检出模块旧JSON别名压过已发布Source，以及删除后不恢复module fallback。
#[tokio::test]
async fn captured_source_keys_precede_module_overlay_and_delete_restores_fallback() -> Result<()> {
    let source = Arc::new(Source::new(session(&[
        ("en", "*", None, r#"{"label":"database global"}"#),
        (
            "en",
            "*",
            Some("ns"),
            r#"{"label":"database namespaced","empty":""}"#,
        ),
    ])?));
    let service = Translator::from_loader(TranslationConfig::default(), source.clone())
        .await?
        .with_lookup(TranslationLookup::Catalog);
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    std::fs::write(
        directory.path().join("en.json"),
        r#"{"label":"module fallback","empty":"module nonempty"}"#,
    )
    .map_err(Error::internal)?;
    service.add_namespace(
        "ns",
        directory.path(),
        directory.path().join("not-published"),
    )?;
    assert_eq!(text(&service, "en", "label")?, "database global");
    assert_eq!(text(&service, "en", "*.label")?, "*.label");
    assert_eq!(text(&service, "en", "ns::label")?, "database namespaced");
    assert_eq!(text(&service, "en", "ns::empty")?, "");
    *source.snapshot.write().map_err(|_| catalog_error())? = session(&[])?;
    service.refresh().await?;
    assert_eq!(text(&service, "en", "label")?, "module fallback");
    assert_eq!(text(&service, "en", "ns::label")?, "module fallback");
    assert_eq!(text(&service, "en", "ns::empty")?, "module nonempty");
    Ok(())
}

// 检出真实请求语言异步边界未刷新，或故障丢失先前已验证目录。
#[tokio::test]
async fn request_locale_boundary_refreshes_and_failure_keeps_published_catalog() -> Result<()> {
    let source = Arc::new(Source::new(session(&[(
        "en",
        "*",
        None,
        r#"{"label":"old"}"#,
    )])?));
    let service = Translator::from_loader(TranslationConfig::default(), source.clone()).await?;
    *source.snapshot.write().map_err(|_| catalog_error())? =
        session(&[("en", "*", None, r#"{"label":"new"}"#)])?;
    with_request_locale(service.clone(), None, async {
        assert_eq!(text(&service, "en", "label")?, "new");
        Ok::<_, Error>(())
    })
    .await?;
    source.fail.store(true, Ordering::SeqCst);
    with_request_locale(service.clone(), None, async {
        assert_eq!(text(&service, "en", "label")?, "new");
        Ok::<_, Error>(())
    })
    .await?;
    assert!(service.refresh_status()?.last_error_code.is_some());
    Ok(())
}

#[tokio::test]
async fn loader_error_destructor_observes_published_refresh_failure() -> Result<()> {
    use std::sync::{Mutex, Weak};

    #[derive(Debug)]
    struct ErrorDropProbe {
        translator: Weak<Translator>,
        saw_completed_failure: Arc<AtomicBool>,
    }
    impl std::fmt::Display for ErrorDropProbe {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("翻译来源测试错误")
        }
    }
    impl std::error::Error for ErrorDropProbe {}
    impl Drop for ErrorDropProbe {
        fn drop(&mut self) {
            let completed = self
                .translator
                .upgrade()
                .and_then(|translator| translator.refresh_status().ok())
                .is_some_and(|status| {
                    !status.refreshing
                        && status.last_error_code == Some("i18n_source_refresh_failed")
                });
            self.saw_completed_failure
                .store(completed, Ordering::SeqCst);
        }
    }
    struct Loader(Mutex<Option<ErrorDropProbe>>);
    impl TranslationLoader for Loader {
        fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
            Box::pin(async move {
                let probe = self.0.lock().map_err(|_| catalog_error())?.take();
                if let Some(probe) = probe {
                    return Err(Error::internal(probe));
                }
                session(&[("en", "*", None, r#"{"key":"old"}"#)])
            })
        }
    }

    let loader = Arc::new(Loader(Mutex::new(None)));
    let translator =
        Arc::new(Translator::from_loader(TranslationConfig::default(), loader.clone()).await?);
    let observed = Arc::new(AtomicBool::new(false));
    *loader.0.lock().map_err(|_| catalog_error())? = Some(ErrorDropProbe {
        translator: Arc::downgrade(&translator),
        saw_completed_failure: Arc::clone(&observed),
    });
    let Err(error) = translator.refresh().await else {
        return Err(catalog_error());
    };
    assert_eq!(error.code(), "i18n_source_refresh_failed");
    assert!(
        observed.load(Ordering::SeqCst),
        "Loader错误析构前必须发布本次失败状态"
    );
    Ok(())
}
