//! 外部长期句柄必须在构造完成后使用新根上下文，不能逃逸构造约束。

use super::*;

struct Handle(Resolver);

#[tokio::test]
async fn root_resolution_rejects_active_factory_and_uses_current_root_after_scope_closes()
-> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(7_u8));
    builder.scoped::<Handle, _>(|resolver| {
        Box::pin(async move {
            let error = resolver.resolve_root::<u8>().await.err().ok_or_else(|| {
                Error::custom("test_expected_failure", "活跃工厂应拒绝根上下文逃逸")
            })?;
            assert_eq!(error.code(), "root_resolution_during_construction");
            Ok(Arc::new(Handle(resolver)))
        })
    });
    let container = builder.build();
    let scope = container.scope()?;
    let handle = scope.resolve::<Handle>().await?;
    scope.close();
    assert_eq!(*handle.0.resolve_root::<u8>().await?, 7);
    container.instance(Arc::new(9_u8)).await?;
    assert_eq!(*handle.0.resolve_root::<u8>().await?, 9);
    container
        .shutdown_root(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await?;
    let error = handle
        .0
        .resolve_root::<u8>()
        .await
        .err()
        .ok_or_else(|| Error::custom("test_expected_failure", "根关闭后应拒绝解析"))?;
    assert_eq!(error.code(), "container_closed");
    Ok(())
}

#[tokio::test]
async fn singleton_cannot_escape_cycle_or_captive_scope_checks_through_root_resolution()
-> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.scoped::<u8, _>(|_| Box::pin(async { Ok(Arc::new(7)) }));
    builder.singleton::<Handle, _>(|resolver| {
        Box::pin(async move {
            for code in [
                resolver
                    .resolve_root::<Handle>()
                    .await
                    .err()
                    .map(|error| error.code().to_owned()),
                resolver
                    .resolve_root::<u8>()
                    .await
                    .err()
                    .map(|error| error.code().to_owned()),
            ] {
                assert_eq!(code.as_deref(), Some("root_resolution_during_construction"));
            }
            let error = resolver.resolve::<u8>().await.err().ok_or_else(|| {
                Error::custom("test_expected_failure", "标准解析仍须拒绝捕获scoped服务")
            })?;
            assert_eq!(error.code(), "captive_scope_dependency");
            Ok(Arc::new(Handle(resolver)))
        })
    });
    let container = builder.build();
    let _ = container.resolve::<Handle>().await?;
    container
        .shutdown_root(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}
