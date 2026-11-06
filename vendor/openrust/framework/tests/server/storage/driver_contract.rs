//! 验证下游应用仅使用公开 API 即可注册并调用自定义存储 driver。

use async_trait::async_trait;
use bytes::Bytes;
use figment::providers::Format;
use futures_util::{StreamExt, stream};
use openrust::core::Error;
use openrust::core::Result;
use openrust::filesystem::FilesystemAdapter;
use openrust::filesystem::FilesystemManager;
use openrust::filesystem::FilesystemServiceProvider;
use openrust::filesystem::StorageBackend;
use openrust::filesystem::StorageDriver;
use openrust::filesystem::StorageDriverConfig;
use openrust::filesystem::StorageMetadata;
use openrust::filesystem::StoragePath;
use openrust::filesystem::StorageReader;
use openrust::filesystem::StorageStream;
use openrust::filesystem::StoredObject;
use openrust::filesystem::UploadCancellation;
use openrust::filesystem::UploadOptions;
use openrust::filesystem::Visibility;
use openrust::foundation::ProviderBuilder;
use openrust::support::BoxFuture;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[tokio::test]
async fn consumer_registers_custom_named_disks_without_private_framework_access()
-> Result<(), Box<dyn std::error::Error>> {
    let figment = figment::Figment::new().merge(figment::providers::Toml::string(
        "[storage]\ndefault_disk = 'primary'\n[storage.disks.primary]\ndriver = 'custom-memory'\nnamespace = 'primary'\n[storage.disks.archive]\ndriver = 'custom-memory'\nnamespace = 'archive'\n",
    ));
    let mut builder = ProviderBuilder::new();
    builder.provider(FilesystemServiceProvider::new().with_driver(MemoryDriver));
    let app = openrust::Application::from_provider_runtime(builder.start(figment).await?)?;
    let storage = app.state().resolve::<FilesystemManager>().await?;
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
    assert_eq!(
        primary
            .visibility(&key)
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("storage_capability_unsupported".to_owned())
    );
    assert_eq!(
        primary
            .set_visibility(&key, Visibility::Public)
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("storage_capability_unsupported".to_owned())
    );
    let public = primary
        .put_stream(
            key.clone(),
            None,
            Box::pin(stream::iter([Ok(Bytes::from_static(b"rejected"))])),
            UploadOptions {
                visibility: Some(Visibility::Public),
                ..Default::default()
            },
            None,
        )
        .await;
    assert_eq!(
        public.err().map(|error| error.code().to_owned()),
        Some("storage_capability_unsupported".to_owned())
    );
    let private = primary
        .put_stream(
            key.clone(),
            None,
            Box::pin(stream::iter([Ok(Bytes::from_static(b"rejected"))])),
            UploadOptions {
                visibility: Some(Visibility::Private),
                ..Default::default()
            },
            None,
        )
        .await;
    assert_eq!(
        private.err().map(|error| error.code().to_owned()),
        Some("storage_capability_unsupported".to_owned())
    );
    assert_eq!(primary.stat(&key).await?.size, 13);
    assert!(archive.stat(&key).await.is_err());
    assert_eq!(read_object(&primary, &key).await?, b"custom driver");
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
            assert_eq!(
                read_object(&primary, &tenant_path).await?,
                payload.as_bytes()
            );
            Ok::<_, Error>(())
        })
        .await??;
    }
    assert!(primary.stat(&tenant_path).await.is_err());
    primary.delete(&key).await?;
    assert!(primary.stat(&key).await.is_err());
    Ok(())
}

async fn read_object(disk: &FilesystemAdapter, path: &StoragePath) -> Result<Vec<u8>> {
    let mut reader = disk.open(path, None).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.body.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(bytes)
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
        visibility: Option<Visibility>,
        mut body: StorageStream,
        cancellation: UploadCancellation,
    ) -> Result<StorageMetadata> {
        if visibility.is_some() {
            return Err(Error::custom(
                "storage_capability_unsupported",
                "custom memory driver does not support visibility",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = body.next().await {
            bytes.extend_from_slice(&chunk?);
        }
        let bytes = Bytes::from(bytes);
        let size = bytes.len() as u64;
        let publication = cancellation.begin_publication()?;
        let Ok(mut objects) = self.objects.lock() else {
            publication.fail();
            return Err(Error::custom(
                "test_lock_poisoned",
                "test store lock was poisoned",
            ));
        };
        objects.insert(
            path.as_str().to_owned(),
            (bytes, content_type.map(str::to_owned)),
        );
        publication.complete();
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum PublicationMode {
    Complete,
    Fail,
    DropToken,
    Uncertain,
    ReturnOkUnfinished,
}

#[derive(Clone)]
struct ControlledGate {
    before_reached: Arc<tokio::sync::Semaphore>,
    before_release: Arc<tokio::sync::Semaphore>,
    after_reached: Arc<tokio::sync::Semaphore>,
    after_release: Arc<tokio::sync::Semaphore>,
    finished: Arc<tokio::sync::Semaphore>,
    mode: PublicationMode,
    double_claim: bool,
    second_claim_rejected: Arc<AtomicBool>,
    early_error: bool,
    return_error_release: Arc<tokio::sync::Semaphore>,
}

impl ControlledGate {
    fn new(mode: PublicationMode) -> Self {
        let semaphore = || Arc::new(tokio::sync::Semaphore::new(0));
        Self {
            before_reached: semaphore(),
            before_release: semaphore(),
            after_reached: semaphore(),
            after_release: semaphore(),
            finished: semaphore(),
            mode,
            double_claim: false,
            second_claim_rejected: Arc::new(AtomicBool::new(false)),
            early_error: false,
            return_error_release: semaphore(),
        }
    }
}

#[derive(Default)]
struct ControlledBackend {
    memory: Arc<MemoryBackend>,
    gate: std::sync::Mutex<Option<ControlledGate>>,
}

struct ControlledDriver(Arc<ControlledBackend>);

impl StorageDriver for ControlledDriver {
    fn name(&self) -> &'static str {
        "controlled-memory"
    }

    fn build<'a>(
        &'a self,
        _config: &'a StorageDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn StorageBackend>>> {
        Box::pin(async move { Ok(Arc::clone(&self.0) as Arc<dyn StorageBackend>) })
    }
}

#[async_trait]
impl StorageBackend for ControlledBackend {
    async fn put(
        &self,
        path: &StoragePath,
        content_type: Option<&str>,
        visibility: Option<Visibility>,
        mut body: StorageStream,
        cancellation: UploadCancellation,
    ) -> Result<StorageMetadata> {
        if visibility.is_some() {
            return Err(Error::custom(
                "storage_capability_unsupported",
                "controlled memory driver does not support visibility",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = body.next().await {
            bytes.extend_from_slice(&chunk?);
        }
        let bytes = Bytes::from(bytes);
        let path = path.as_str().to_owned();
        let content_type = content_type.map(str::to_owned);
        let gate = self
            .gate
            .lock()
            .map_err(|_| Error::custom("test_lock_poisoned", "gate lock was poisoned"))?
            .take();
        let memory = Arc::clone(&self.memory);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let worker_gate = gate.clone();
        tokio::spawn(async move {
            let result = async {
                if let Some(gate) = &worker_gate {
                    gate.before_reached.add_permits(1);
                    let _ = gate.before_release.acquire().await;
                }
                let publication = cancellation.begin_publication()?;
                if let Some(gate) = &worker_gate {
                    gate.after_reached.add_permits(1);
                    let _ = gate.after_release.acquire().await;
                    if gate.double_claim {
                        gate.second_claim_rejected
                            .store(cancellation.begin_publication().is_err(), Ordering::Release);
                    }
                    if gate.mode == PublicationMode::ReturnOkUnfinished {
                        drop(publication);
                        return Ok(StorageMetadata {
                            size: bytes.len() as u64,
                            content_type,
                            ..Default::default()
                        });
                    }
                    if gate.mode == PublicationMode::Fail {
                        publication.fail();
                        return Err(Error::custom("controlled_commit_failed", "commit failed"));
                    }
                    if gate.mode == PublicationMode::DropToken {
                        drop(publication);
                        return Err(Error::custom(
                            "controlled_worker_interrupted",
                            "publisher stopped after claiming publication",
                        ));
                    }
                    if gate.mode == PublicationMode::Uncertain {
                        publication.uncertain();
                        return Err(Error::custom(
                            "controlled_transport_error",
                            "transport stopped after final commit request",
                        ));
                    }
                }
                let Ok(mut objects) = memory.objects.lock() else {
                    publication.fail();
                    return Err(Error::custom(
                        "test_lock_poisoned",
                        "object lock was poisoned",
                    ));
                };
                let size = bytes.len() as u64;
                objects.insert(path, (bytes, content_type.clone()));
                publication.complete();
                Ok(StorageMetadata {
                    size,
                    content_type,
                    ..Default::default()
                })
            }
            .await;
            if let Some(gate) = &worker_gate {
                gate.finished.add_permits(1);
            }
            let _ = sender.send(result);
        });
        if let Some(gate) = &gate
            && gate.early_error
        {
            let _ = gate.return_error_release.acquire().await;
            return Err(Error::custom(
                "controlled_precommit_error",
                "driver returned before detached publisher finished",
            ));
        }
        receiver
            .await
            .map_err(|_| Error::custom("controlled_worker_lost", "publisher task stopped"))?
    }

    async fn open(&self, path: &StoragePath, version: Option<&str>) -> Result<StorageReader> {
        self.memory.open(path, version).await
    }

    async fn stat(&self, path: &StoragePath) -> Result<StorageMetadata> {
        self.memory.stat(path).await
    }

    async fn delete(&self, path: &StoragePath) -> Result<()> {
        self.memory.delete(path).await
    }
}

async fn controlled_disk(
    path: &str,
) -> Result<(Arc<ControlledBackend>, FilesystemAdapter, StoragePath)> {
    let backend = Arc::new(ControlledBackend::default());
    let mut builder = ProviderBuilder::new();
    builder.provider(
        FilesystemServiceProvider::new().with_driver(ControlledDriver(Arc::clone(&backend))),
    );
    let app = openrust::Application::from_provider_runtime(
        builder
            .start(
                figment::Figment::new().merge(figment::providers::Toml::string(
                    "[storage]\n[storage.disks.local]\ndriver = 'controlled-memory'\n",
                )),
            )
            .await?,
    )?;
    let disk = app
        .state()
        .resolve::<FilesystemManager>()
        .await?
        .default_disk()
        .await?;
    let path = StoragePath::new(path)?;
    disk.put_stream(
        path.clone(),
        None,
        Box::pin(stream::iter([Ok(Bytes::from_static(b"old"))])),
        UploadOptions::default(),
        None,
    )
    .await?;
    Ok((backend, disk, path))
}

fn spawn_upload(
    disk: &FilesystemAdapter,
    path: &StoragePath,
    payload: Bytes,
) -> tokio::task::JoinHandle<Result<StoredObject>> {
    let disk = disk.clone();
    let path = path.clone();
    tokio::spawn(async move {
        disk.put_stream(
            path,
            None,
            Box::pin(stream::iter([Ok(payload)])),
            UploadOptions::default(),
            None,
        )
        .await
    })
}

fn current(backend: &ControlledBackend, path: &StoragePath) -> Result<Bytes> {
    backend
        .memory
        .objects
        .lock()
        .map_err(|_| Error::custom("test_lock_poisoned", "object lock was poisoned"))?
        .get(path.as_str())
        .map(|(bytes, _)| bytes.clone())
        .ok_or_else(|| Error::custom("test_object_missing", "target disappeared"))
}

#[tokio::test]
async fn external_driver_arbitrates_cancel_publish_and_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let (backend, disk, path) = controlled_disk("publish/object.bin").await?;
    for (replacement, cancel_before, mode) in [
        (b"cancelled".as_slice(), true, PublicationMode::Complete),
        (b"committed".as_slice(), false, PublicationMode::Complete),
        (b"failed".as_slice(), false, PublicationMode::Fail),
        (b"dropped".as_slice(), false, PublicationMode::DropToken),
        (b"unknown".as_slice(), false, PublicationMode::Uncertain),
    ] {
        let gate = ControlledGate::new(mode);
        *backend
            .gate
            .lock()
            .map_err(|_| Error::custom("test_lock_poisoned", "gate lock was poisoned"))? =
            Some(gate.clone());
        let mut upload = spawn_upload(&disk, &path, Bytes::copy_from_slice(replacement));
        let _ = gate.before_reached.acquire().await?;
        if cancel_before {
            upload.abort();
            let _ = (&mut upload).await;
            gate.before_release.add_permits(1);
        } else {
            gate.before_release.add_permits(1);
            let _ = gate.after_reached.acquire().await?;
            if mode == PublicationMode::Complete {
                upload.abort();
                let _ = (&mut upload).await;
            }
            gate.after_release.add_permits(1);
        }
        let _ = gate.finished.acquire().await?;
        if mode != PublicationMode::Complete {
            assert_eq!(
                (&mut upload)
                    .await?
                    .err()
                    .map(|error| error.code().to_owned()),
                Some(
                    if mode == PublicationMode::Fail {
                        "controlled_commit_failed"
                    } else {
                        "storage_publication_uncertain"
                    }
                    .to_owned()
                )
            );
        }
        assert_eq!(
            read_object(&disk, &path).await?,
            if cancel_before {
                b"old".as_slice()
            } else {
                b"committed".as_slice()
            }
        );
    }
    Ok(())
}

#[tokio::test]
async fn external_driver_rejects_duplicate_claim_and_arbitrates_early_error()
-> Result<(), Box<dyn std::error::Error>> {
    let (backend, disk, path) = controlled_disk("publish/claim-races.bin").await?;

    let mut cancel_wins = ControlledGate::new(PublicationMode::Complete);
    cancel_wins.early_error = true;
    *backend
        .gate
        .lock()
        .map_err(|_| Error::custom("test_lock_poisoned", "gate lock was poisoned"))? =
        Some(cancel_wins.clone());
    let upload = spawn_upload(&disk, &path, Bytes::from_static(b"late rejected"));
    let _ = cancel_wins.before_reached.acquire().await?;
    cancel_wins.return_error_release.add_permits(1);
    assert_eq!(
        upload.await?.err().map(|error| error.code().to_owned()),
        Some("controlled_precommit_error".to_owned())
    );
    cancel_wins.before_release.add_permits(1);
    let _ = cancel_wins.finished.acquire().await?;
    assert_eq!(current(&backend, &path)?, Bytes::from_static(b"old"));

    let mut publish_wins = ControlledGate::new(PublicationMode::Complete);
    publish_wins.early_error = true;
    *backend
        .gate
        .lock()
        .map_err(|_| Error::custom("test_lock_poisoned", "gate lock was poisoned"))? =
        Some(publish_wins.clone());
    let upload = spawn_upload(&disk, &path, Bytes::from_static(b"committed despite error"));
    let _ = publish_wins.before_reached.acquire().await?;
    publish_wins.before_release.add_permits(1);
    let _ = publish_wins.after_reached.acquire().await?;
    publish_wins.return_error_release.add_permits(1);
    assert_eq!(
        upload.await?.err().map(|error| error.code().to_owned()),
        Some("storage_publication_uncertain".to_owned())
    );
    publish_wins.after_release.add_permits(1);
    let _ = publish_wins.finished.acquire().await?;
    assert_eq!(
        current(&backend, &path)?,
        Bytes::from_static(b"committed despite error")
    );

    let mut duplicate = ControlledGate::new(PublicationMode::Complete);
    duplicate.double_claim = true;
    *backend
        .gate
        .lock()
        .map_err(|_| Error::custom("test_lock_poisoned", "gate lock was poisoned"))? =
        Some(duplicate.clone());
    let upload = spawn_upload(&disk, &path, Bytes::from_static(b"one claim only"));
    let _ = duplicate.before_reached.acquire().await?;
    duplicate.before_release.add_permits(1);
    let _ = duplicate.after_reached.acquire().await?;
    duplicate.after_release.add_permits(1);
    upload.await??;
    let _ = duplicate.finished.acquire().await?;
    assert!(duplicate.second_claim_rejected.load(Ordering::Acquire));
    assert_eq!(
        current(&backend, &path)?,
        Bytes::from_static(b"one claim only")
    );

    let unfinished = ControlledGate::new(PublicationMode::ReturnOkUnfinished);
    *backend
        .gate
        .lock()
        .map_err(|_| Error::custom("test_lock_poisoned", "gate lock was poisoned"))? =
        Some(unfinished.clone());
    let upload = spawn_upload(&disk, &path, Bytes::from_static(b"must not publish"));
    let _ = unfinished.before_reached.acquire().await?;
    unfinished.before_release.add_permits(1);
    let _ = unfinished.after_reached.acquire().await?;
    unfinished.after_release.add_permits(1);
    assert_eq!(
        upload.await?.err().map(|error| error.code().to_owned()),
        Some("storage_publication_uncertain".to_owned())
    );
    let _ = unfinished.finished.acquire().await?;
    assert_eq!(
        current(&backend, &path)?,
        Bytes::from_static(b"one claim only")
    );
    Ok(())
}
