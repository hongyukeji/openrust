use std::sync::Arc;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::views::{DefaultViews, VIEWS};

pub(crate) struct PaginationServiceProvider;

impl ServiceProvider for PaginationServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .instance(Arc::new(DefaultViews::default()))?;
            app.container()
                .instance(Arc::new(super::pagination_state::PaginationState::default()))?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let assets: Vec<_> = VIEWS
                .iter()
                .map(|(name, source)| {
                    (
                        format!("resources/views/vendor/pagination/{name}.blade.html"),
                        source.as_bytes(),
                    )
                })
                .collect();
            let files: Vec<_> = assets
                .iter()
                .map(|(path, bytes)| (path.as_str(), *bytes))
                .collect();
            self.publishes(app, &files, "laravel-pagination")
        })
    }
}
