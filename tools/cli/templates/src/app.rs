use std::sync::Arc;
use openrust::core::{BootContext, BoxFuture, RegisterContext, ServiceProvider};
use openrust::prelude::*;

use crate::routes;

struct ReadinessProvider;
impl ServiceProvider for ReadinessProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(openrust::core::Readiness::default()));
        Ok(())
    }
}

struct RoutesProvider;
impl ServiceProvider for RoutesProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.routes(routes::register)?; Ok(()) })
    }
}

pub struct {{type_name}};

impl Application for {{type_name}} {
    type Config = NoConfig;

    fn providers(_config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers: Vec<Box<dyn ServiceProvider>> = vec![
            Box::new(openrust::http::HttpServiceProvider),
            Box::new(ReadinessProvider),
            Box::new(openrust::view::ViewServiceProvider),
            Box::new(openrust::i18n::I18nServiceProvider::default()),
            Box::new(openrust::cache::CacheServiceProvider::new()),
            Box::new(openrust::mail::MailerServiceProvider),
            Box::new(openrust::sms::SmsServiceProvider),
        ];{{database_providers}}{{encryption_provider}}
        providers.push(Box::new(RoutesProvider));
        providers
    }
}
