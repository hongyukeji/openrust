//! 验证下游应用仅使用公开 API 即可注册并调用自定义存储 driver。

use async_trait::async_trait;
use bytes::Bytes;
use figment::providers::Format;
use futures_util::{StreamExt, stream};
use openrust::core::{BoxFuture, Error, ProviderBuilder, Result};
use openrust::storage::{
    Storage, StorageBackend, StorageDriver, StorageDriverConfig, StorageMetadata, StoragePath,
    StorageReader, StorageServiceProvider, StorageStream, UploadCancellation, UploadOptions,
};
use std::{collections::BTreeMap, sync::Arc};

#[tokio::test]
async fn consumer_registers_custom_named_disks_without_private_framework_access()
-> Result<(), Box<dyn std::error::Error>> {
    let figment = figment::Figment::new().merge(figment::providers::Toml::string(
        "[storage]\ndefault_disk = 'primary'\n[storage.disks.primary]\ndriver = 'custom-memory'\nnamespace = 'primary'\n[storage.disks.archive]\ndriver = 'custom-memory'\nnamespace = 'archive'\n",
    ));
    let mut builder = ProviderBuilder::new();
    builder.provider(StorageServiceProvider::new().with_driver(MemoryDriver));
    let app = openrust::App::from_provider_runtime(builder.start(figment).await?)?;
    let storage = app.state().resolve::<Storage>().await?;
    let primary = storage.default_disk().await?;
    let archive = storage.disk("archive").await?;
    let key = StoragePath::new("objects/report.bin")?;
    primary
        .put_stream(
            key.clone(),
            Some("application/octet-stream".into()),
            Box::pin(stream::iter([Ok(Bytes::from_static(b"custom driver"))])) as StorageStream,
            UploadOptions::default(),
            None,
        )
        .await?;
    assert_eq!(primary.stat(&key).await?.size, 13);
    assert!(archive.stat(&key).await.is_err());
    let mut reader = primary.open(&key, None).await?;
    let mut output = Vec::new();
    while let Some(chunk) = reader.body.next().await {
        output.extend_from_slice(&chunk?);
    }
    assert_eq!(output, b"custom driver");
    let tenant_path = StoragePath::new("objects/shared.bin")?;
    for (tenant, payload) in [("tenant-a", "A"), ("tenant-b", "B")] {
        openrust::tenant::TenantContext::scope(openrust::tenant::TenantId::new(tenant)?, async {
            primary
                .put_stream(
                    tenant_path.clone(),
                    None,
                    Box::pin(stream::iter([Ok(Bytes::from(payload.as_bytes()))])) as StorageStream,
                    UploadOptions::default(),
                    None,
                )
                .await?;
            let mut reader = primary.open(&tenant_path, None).await?;
            let mut bytes = Vec::new();
            while let Some(chunk) = reader.body.next().await {
                bytes.extend_from_slice(&chunk?);
            }
            assert_eq!(bytes, payload.as_bytes());
            Ok::<_, Error>(())
        })
        .await??;
    }
    assert!(primary.stat(&tenant_path).await.is_err());
    primary.delete(&key).await?;
    assert!(primary.stat(&key).await.is_err());
    Ok(())
}

struct MemoryDriver;

impl StorageDriver for MemoryDriver {
    fn name(&self) -> &'static str {
        "custom-memory"
    }

    fn build<'a>(
        &'a self,
        config: &'a StorageDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
        Box::pin(async move {
            config.reject_unknown_options(&["namespace"])?;
            let _namespace: String = config.option("namespace")?.ok_or_else(|| {
                Error::custom("namespace_missing", "driver namespace is required")
            })?;
            Ok(Arc::new(MemoryBackend::default()) as Arc<dyn StorageBackend>)
        })
    }
}

#[derive(Default)]
struct MemoryBackend {
    objects: std::sync::Mutex<BTreeMap<String, (Bytes, Option<String>)>>,
}

#[async_trait]
impl StorageBackend for MemoryBackend {
    async fn put(
        &self,
        path: &StoragePath,
        content_type: Option<&str>,
        mut body: StorageStream,
        _cancellation: UploadCancellation,
    ) -> Result<StorageMetadata> {
        let mut bytes = Vec::new();
        while let Some(chunk) = body.next().await {
            bytes.extend_from_slice(&chunk?);
        }
        let bytes = Bytes::from(bytes);
        let size = bytes.len() as u64;
        self.objects
            .lock()
            .map_err(|_| Error::custom("test_lock_poisoned", "test store lock was poisoned"))?
            .insert(
                path.as_str().to_owned(),
                (bytes, content_type.map(str::to_owned)),
            );
        Ok(StorageMetadata {
            size,
            content_type: content_type.map(str::to_owned),
            ..Default::default()
        })
    }

    async fn open(&self, path: &StoragePath, version: Option<&str>) -> Result<StorageReader> {
        if version.is_some() {
            return Err(Error::custom(
                "version_unsupported",
                "custom driver has no versions",
            ));
        }
        let (bytes, content_type) = self
            .objects
            .lock()
            .map_err(|_| Error::custom("test_lock_poisoned", "test store lock was poisoned"))?
            .get(path.as_str())
            .cloned()
            .ok_or_else(|| Error::NotFound("对象不存在".into()))?;
        let size = bytes.len() as u64;
        Ok(StorageReader {
            metadata: StorageMetadata {
                size,
                content_type,
                ..Default::default()
            },
            body: Box::pin(stream::iter([Ok(bytes)])),
        })
    }

    async fn stat(&self, path: &StoragePath) -> Result<StorageMetadata> {
        let objects = self
            .objects
            .lock()
            .map_err(|_| Error::custom("test_lock_poisoned", "test store lock was poisoned"))?;
        let (bytes, content_type) = objects
            .get(path.as_str())
            .ok_or_else(|| Error::NotFound("对象不存在".into()))?;
        Ok(StorageMetadata {
            size: bytes.len() as u64,
            content_type: content_type.clone(),
            ..Default::default()
        })
    }

    async fn delete(&self, path: &StoragePath) -> Result<()> {
        self.objects
            .lock()
            .map_err(|_| Error::custom("test_lock_poisoned", "test store lock was poisoned"))?
            .remove(path.as_str());
        Ok(())
    }
}
