use std::sync::Arc;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{
    ConnectionResolverInterface, DatabaseManager, test_mysql::MySqlTestOwners,
    test_query_counts::TestQueryCounts,
};

/// Registers database services with the application container.
pub struct DatabaseServiceProvider;

impl ServiceProvider for DatabaseServiceProvider {
    fn register<'a>(&'a mut self, application: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let root = application.base_path().to_path_buf();
            let owner = Arc::downgrade(&application.container_handle());
            let container = application.container();
            container.singleton_named("db", move |container| {
                let config = container.get::<dyn crate::contracts::config::Repository>("config")?;
                let key = crate::container::ServiceKey::typed::<MySqlTestOwners>();
                let mysql_test_owners = if container.has(key.clone()) {
                    Some(container.get::<MySqlTestOwners>(key)?)
                } else {
                    None
                };
                let key = crate::container::ServiceKey::typed::<TestQueryCounts>();
                let counts = if container.has(key.clone()) {
                    Some(Arc::downgrade(&container.get::<TestQueryCounts>(key)?))
                } else {
                    None
                };
                Ok(Arc::new(
                    DatabaseManager::new(config, root.clone())
                        .with_container(owner.clone())
                        .with_test_services(mysql_test_owners, counts),
                ))
            })?;
            container.alias(
                "db",
                crate::container::ServiceKey::typed::<DatabaseManager>(),
            )?;
            container.singleton::<dyn ConnectionResolverInterface, _>(|container| {
                let manager = container.get::<DatabaseManager>("db")?;
                Ok(manager)
            })?;
            container.singleton::<super::eloquent::ModelEvents, _>(|_| {
                Ok(Arc::new(super::eloquent::ModelEvents::default()))
            })?;
            container.singleton::<super::eloquent::ModelPolicies, _>(|_| {
                Ok(Arc::new(super::eloquent::ModelPolicies::default()))
            })?;
            container.singleton::<super::eloquent::RelationResolvers, _>(|_| {
                Ok(Arc::new(super::eloquent::RelationResolvers::default()))
            })?;
            container.singleton::<super::eloquent::relations::MorphMap, _>(|_| {
                Ok(Arc::new(super::eloquent::relations::MorphMap::default()))
            })?;
            container.singleton_if::<super::seeder::SeederRegistry, _>(|_| {
                Ok(Arc::new(super::seeder::SeederRegistry::default()))
            })?;
            Ok(())
        })
    }
}
