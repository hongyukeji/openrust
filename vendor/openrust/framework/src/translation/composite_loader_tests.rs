//! 同一资源覆盖与语言候选隔离，组合不自行执行fallback。
use super::*;
use crate::translation::Locale;
use serde_json::json;

struct Fixed(BTreeMap<TranslationResource, TranslationLines>);
impl TranslationLoader for Fixed {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async {
            Ok(Arc::new(MaterializedSession::new(self.0.clone())?)
                as Arc<dyn TranslationLoadSession>)
        })
    }
}
fn resource(locale: &str, namespace: Option<&str>) -> Result<TranslationResource> {
    Ok(TranslationResource {
        locale: Locale::new(locale)?,
        group: "*".into(),
        namespace: namespace.map(str::to_owned),
    })
}
#[tokio::test]
async fn later_sources_override_only_same_locale_resource_and_keep_empty() -> Result<()> {
    let en = resource("en", None)?;
    let fr = resource("fr", None)?;
    let shop = resource("fr", Some("shop"))?;
    let file = Fixed(BTreeMap::from([
        (
            en.clone(),
            BTreeMap::from([
                ("key".into(), json!("file-en")),
                ("empty".into(), json!("original")),
            ]),
        ),
        (
            fr.clone(),
            BTreeMap::from([("key".into(), json!("fichier-fr"))]),
        ),
        (
            shop.clone(),
            BTreeMap::from([("key".into(), json!("boutique"))]),
        ),
    ]));
    let database = Fixed(BTreeMap::from([(
        en.clone(),
        BTreeMap::from([("key".into(), json!("db-en")), ("empty".into(), json!(""))]),
    )]));
    let loader = CompositeTranslationLoader::new(vec![Arc::new(file), Arc::new(database)]);
    let session = loader.begin_snapshot().await?;
    assert_eq!(
        session.load(&en.locale, "*", None)?.get("key"),
        Some(&json!("db-en"))
    );
    assert_eq!(
        session.load(&en.locale, "*", None)?.get("empty"),
        Some(&json!(""))
    );
    assert_eq!(
        session.load(&fr.locale, "*", None)?.get("key"),
        Some(&json!("fichier-fr"))
    );
    assert_eq!(
        session.load(&fr.locale, "*", Some("shop"))?.get("key"),
        Some(&json!("boutique"))
    );
    assert_eq!(
        session.load(&Locale::new("de")?, "*", None)?,
        BTreeMap::new()
    );
    assert!(!loader.needs_refresh().await?);
    Ok(())
}
struct Failed;
impl TranslationLoader for Failed {
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>> {
        Box::pin(async { Err(super::super::loader::invalid_resource()) })
    }
}
#[tokio::test]
async fn any_source_failure_prevents_partial_composite_publication() -> Result<()> {
    let loader =
        CompositeTranslationLoader::new(vec![Arc::new(Fixed(BTreeMap::new())), Arc::new(Failed)]);
    assert!(loader.begin_snapshot().await.is_err());
    let empty = CompositeTranslationLoader::new(Vec::new())
        .begin_snapshot()
        .await?;
    assert_eq!(empty.resources(), &[]);
    Ok(())
}
