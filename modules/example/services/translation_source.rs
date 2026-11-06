//! 运维命令经真实业务服务管理文案；不开放匿名HTTP写入或独立翻译内核。
use openrust::console::{Command, print};
use openrust::foundation::{Application, BootContext, RegisterContext};
use openrust::support::{BoxFuture, ServiceProvider};
use openrust::translation::{
    Locale, MessageArgs, TranslationLookup, TranslationResource, Translator,
};
use openrust::{Error, Result};
use openrust_translation_database::{DatabaseTranslationLoader, TranslationMutation};
use std::sync::Arc;

pub(crate) struct TranslationSourceExample {
    loader: Arc<DatabaseTranslationLoader>,
    translator: Arc<Translator>,
}
#[derive(serde::Serialize)]
pub(crate) struct WriteReceipt {
    pub committed_revision: u64,
    pub notifications_delivered: bool,
    pub refresh_error_code: Option<String>,
}
impl TranslationSourceExample {
    async fn after_commit(
        &self,
        outcome: openrust_translation_database::TranslationWriteOutcome,
    ) -> WriteReceipt {
        let error = self
            .translator
            .refresh()
            .await
            .err()
            .map(|error| error.code().to_owned());
        WriteReceipt {
            committed_revision: outcome.revision,
            notifications_delivered: outcome.notifications_delivered,
            refresh_error_code: error,
        }
    }
    fn resource(locale: &str) -> Result<TranslationResource> {
        Ok(TranslationResource {
            locale: Locale::new(locale)?,
            group: "*".into(),
            namespace: Some("example-source".into()),
        })
    }
    pub(crate) async fn set(
        &self,
        locale: &str,
        text: &str,
        enabled: bool,
    ) -> Result<WriteReceipt> {
        let outcome = self
            .loader
            .write(vec![TranslationMutation::Put {
                resource: Self::resource(locale)?,
                key: "label".into(),
                value: serde_json::Value::String(text.into()),
                enabled,
            }])
            .await?;
        Ok(self.after_commit(outcome).await)
    }
    pub(crate) async fn delete(&self, locale: &str) -> Result<WriteReceipt> {
        let outcome = self
            .loader
            .write(vec![TranslationMutation::Delete {
                resource: Self::resource(locale)?,
                key: "label".into(),
            }])
            .await?;
        Ok(self.after_commit(outcome).await)
    }
    pub(crate) fn show(&self, locale: Option<&str>) -> Result<String> {
        let locale = match locale {
            Some(locale) => Locale::new(locale)?,
            None => self.translator.select(None),
        };
        // 运维文案预览明确保留Catalog增强；不改变Container中的普通Translator。
        let catalog_view = self
            .translator
            .as_ref()
            .clone()
            .with_lookup(TranslationLookup::Catalog);
        Ok(catalog_view.translate(&locale, "example-source::label", &MessageArgs::new()))
    }
}
pub(crate) struct TranslationSourceExampleProvider;
impl ServiceProvider for TranslationSourceExampleProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !app.is_bound::<DatabaseTranslationLoader>() {
                return Ok(());
            }
            app.singleton::<TranslationSourceExample, _>(|resolver| {
                Box::pin(async move {
                    Ok(Arc::new(TranslationSourceExample {
                        loader: resolver.resolve::<DatabaseTranslationLoader>().await?,
                        translator: resolver.resolve::<Translator>().await?,
                    }))
                })
            })
            .await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if app.is_bound::<TranslationSourceExample>() {
                let service = app.resolve::<TranslationSourceExample>().await?;
                app.command(TranslationSourceCommand(service));
            }
            Ok(())
        })
    }
}
struct TranslationSourceCommand(Arc<TranslationSourceExample>);
impl Command for TranslationSourceCommand {
    fn name(&self) -> &'static str {
        "example:translation"
    }
    fn about(&self) -> &'static str {
        "管理示例Source文案并查看当前统一翻译结果"
    }
    fn accepts_args(&self) -> bool {
        true
    }
    fn run<'a>(&'a self, _app: &'a Application, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            match args.as_slice() {
                [action] if action == "show" => print(&self.0.show(None)?),
                [action, locale] if action == "show" => print(&self.0.show(Some(locale))?),
                [action, locale, text] if action == "set" => print(
                    &serde_json::to_string(&self.0.set(locale, text, true).await?)
                        .map_err(Error::internal)?,
                ),
                [action, locale] if action == "disable" => print(
                    &serde_json::to_string(&self.0.set(locale, "", false).await?)
                        .map_err(Error::internal)?,
                ),
                [action, locale] if action == "delete" => print(
                    &serde_json::to_string(&self.0.delete(locale).await?)
                        .map_err(Error::internal)?,
                ),
                [action] if action == "refresh" => {
                    self.0.translator.refresh().await?;
                }
                _ => {
                    return Err(Error::custom(
                        "example_translation_arguments",
                        "使用show [locale]、set locale text、disable locale、delete locale或refresh",
                    ));
                }
            }
            Ok(())
        })
    }
}
