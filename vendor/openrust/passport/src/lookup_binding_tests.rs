//! 实际Scope/Resolver的关闭与captive契约，不造临时Resolver或全局上下文。

use super::lookup_test_support::repository_bindings;
use super::*;
use crate::GrantAuthority;
use openrust::{container::Resolver, support::BoxFuture};
use std::sync::atomic::{AtomicUsize, Ordering};

struct DenyAuthority;
impl<'tx> GrantAuthority<TransactionContext<'tx>> for DenyAuthority {
    fn validate<'a>(
        &'a self,
        _db_context: &'a TransactionContext<'tx>,
        _context: &'a GrantValidation<'a>,
        _binding: &'a GrantAuthorityBinding,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async {
            Err(Error::custom(
                "fixture_authority_denied",
                "纯DI夹具不授权写入",
            ))
        })
    }
}
struct EntryClient(ClientRecord);
struct ContextLookup;
impl ClientLookup for ContextLookup {
    fn find<'a>(
        &'a self,
        context: &'a ClientLookupContext<'a>,
        _id: &'a Value,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>> {
        Box::pin(async move { Ok(Some(context.resolve::<EntryClient>().await?.0.clone())) })
    }
}
struct Captive;

#[tokio::test]
async fn same_resolver_scope_shares_actual_model_and_other_scope_is_isolated() -> Result<()> {
    let factory: Arc<dyn ClientModelFactory> = Arc::new(crate::ClientFactory::default());
    let mut builder = repository_bindings(
        Db::default(),
        factory.clone(),
        GrantUserKey::Integer,
        Arc::new(DenyAuthority),
        Arc::new(SystemGrantClock),
        Some(Arc::new(ContextLookup)),
    )?;
    let hydrations = Arc::new(AtomicUsize::new(0));
    let count = hydrations.clone();
    builder.scoped::<EntryClient, _>(move |_| {
        let factory = factory.clone();
        let count = count.clone();
        Box::pin(async move {
            let tenant = count.fetch_add(1, Ordering::SeqCst);
            let model = factory.new_from_builder(Map::from_iter([
                (
                    "id".to_owned(),
                    serde_json::json!("019a4d4b-f21a-7191-bce9-fdc817a165aa"),
                ),
                (
                    "name".to_owned(),
                    serde_json::json!(format!("tenant-{tenant}")),
                ),
                ("revoked".to_owned(), Value::Bool(false)),
            ]))?;
            Ok(Arc::new(EntryClient(model)))
        })
    });
    builder.singleton::<Captive, _>(|resolver: Resolver| {
        Box::pin(async move {
            resolver.resolve::<DatabaseGrantRepository>().await?;
            Ok(Arc::new(Captive))
        })
    });
    let container = builder.build();
    assert!(
        matches!(container.resolve::<Captive>().await, Err(error) if error.code()=="captive_scope_dependency")
    );
    let left = container.scope()?;
    let right = container.scope()?;
    let left_repository = left.resolve::<DatabaseGrantRepository>().await?;
    let first = left_repository
        .client_by_protocol("application-specific-id")
        .await?
        .ok_or_else(binding_invalid)?;
    let second = left_repository
        .client_by_protocol("application-specific-id")
        .await?
        .ok_or_else(binding_invalid)?;
    assert!(Arc::ptr_eq(&first, &second));
    assert!(Arc::ptr_eq(&first, &left.resolve::<EntryClient>().await?.0));
    let right_repository = right.resolve::<DatabaseGrantRepository>().await?;
    let other = right_repository
        .client_by_protocol("application-specific-id")
        .await?
        .ok_or_else(binding_invalid)?;
    assert!(!Arc::ptr_eq(&first, &other));
    assert_ne!(first.name()?, other.name()?);
    assert_eq!(hydrations.load(Ordering::SeqCst), 2);
    let mut raw = first.attributes()?;
    raw.insert("revoked".to_owned(), Value::Bool(true));
    first.set_raw_attributes(raw)?;
    assert!(
        left_repository
            .client_by_protocol("application-specific-id")
            .await?
            .is_none()
    );
    assert!(
        right_repository
            .client_by_protocol("application-specific-id")
            .await?
            .is_some()
    );
    assert_eq!(hydrations.load(Ordering::SeqCst), 2);
    left.close();
    assert!(
        matches!(left_repository.client_by_protocol("application-specific-id").await, Err(error) if error.code()=="scope_closed")
    );
    assert!(
        right_repository
            .client_by_protocol("application-specific-id")
            .await?
            .is_some()
    );
    right.close();
    Ok(())
}
