//! SQL fixture也经真实Container/scoped工厂构造，保弱Resolver及同入口cache。

use super::*;
use openrust::container::ContainerBuilder;
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
use openrust::container::{Container, Scope};

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
pub(super) struct ScopedRepository {
    pub(super) repository: Arc<DatabaseGrantRepository>,
    pub(super) scope: Arc<Scope>,
    pub(super) container: Container,
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
pub(super) async fn scoped_repository(
    db: Db,
    factory: Arc<dyn ClientModelFactory>,
    user_key: GrantUserKey,
    authority: Arc<DatabaseGrantAuthority>,
    clock: Arc<dyn GrantClock>,
    lookup: Option<Arc<dyn ClientLookup>>,
) -> Result<ScopedRepository> {
    let builder = repository_bindings(db, factory, user_key, authority, clock, lookup)?;
    let container = builder.build();
    let scope = Arc::new(container.scope()?);
    let repository = scope.resolve::<DatabaseGrantRepository>().await?;
    Ok(ScopedRepository {
        repository,
        scope,
        container,
    })
}

pub(super) fn repository_bindings(
    db: Db,
    factory: Arc<dyn ClientModelFactory>,
    user_key: GrantUserKey,
    authority: Arc<DatabaseGrantAuthority>,
    clock: Arc<dyn GrantClock>,
    lookup: Option<Arc<dyn ClientLookup>>,
) -> Result<ContainerBuilder> {
    let mut builder = ContainerBuilder::new();
    builder.scoped::<crate::repository::ClientLookupCache, _>(|_| {
        Box::pin(async { Ok(Arc::new(crate::repository::ClientLookupCache::default())) })
    });
    let clients: Arc<dyn ClientLookup> = match lookup {
        Some(lookup) => lookup,
        None => Arc::new(crate::ClientRepository::from_db(
            db.clone(),
            factory.clone(),
        )?),
    };
    builder.instance::<dyn ClientLookup>(clients);
    builder.scoped::<DatabaseGrantRepository, _>(move |resolver| {
        let db = db.clone();
        let factory = factory.clone();
        let authority = authority.clone();
        let clock = clock.clone();
        Box::pin(async move {
            let clients = resolver.resolve::<dyn ClientLookup>().await?;
            Ok(Arc::new(DatabaseGrantRepository {
                db,
                factory,
                user_key,
                authority,
                clock,
                clients,
                resolver,
            }))
        })
    });
    Ok(builder)
}
