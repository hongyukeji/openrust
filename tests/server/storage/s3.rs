//! 验证真实 S3-compatible API、版本绑定 URL 与多段上传。

use bytes::Bytes;
use figment::providers::Format;
use futures_util::{StreamExt, stream};
use openrust::storage::{
    Storage, StoragePath, StorageServiceProvider, StorageStream, UploadInspector, UploadOptions,
};
use openrust::{Error, Result, core::ProviderBuilder};
use std::{sync::Arc, time::Duration};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
#[ignore = "需单独启动已开启 bucket versioning 的真实 S3-compatible 服务后显式运行"]
#[allow(clippy::too_many_lines)]
async fn s3_streams_multipart_cancellation_and_exact_version_presigned_reads() -> TestResult {
    let endpoint = std::env::var("OPENRUST_TEST_S3_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:19000".to_owned());
    let versioned_bucket = std::env::var("OPENRUST_TEST_S3_VERSIONED_BUCKET")
        .unwrap_or_else(|_| "openrust-storage-versioned".to_owned());
    let plain_bucket = std::env::var("OPENRUST_TEST_S3_PLAIN_BUCKET")
        .unwrap_or_else(|_| "openrust-storage-plain".to_owned());
    let access_key =
        std::env::var("OPENRUST_TEST_S3_ACCESS_KEY").unwrap_or_else(|_| "openrust-test".to_owned());
    let secret_key = std::env::var("OPENRUST_TEST_S3_SECRET_KEY")
        .unwrap_or_else(|_| "openrust-test-secret".to_owned());
    let versioned = serde_json::to_string(&versioned_bucket)?;
    let plain = serde_json::to_string(&plain_bucket)?;
    let endpoint = serde_json::to_string(&endpoint)?;
    let access_key = serde_json::to_string(&access_key)?;
    let secret_key = serde_json::to_string(&secret_key)?;
    let figment = figment::Figment::new().merge(figment::providers::Toml::string(&format!(
        "[storage]\nmax_upload_bytes = 33554432\nmax_chunk_bytes = 1048576\ndefault_disk = 'versioned'\n[storage.disks.versioned]\ndriver = 's3'\nbucket = {versioned}\nregion = 'us-east-1'\nendpoint = {endpoint}\naccess_key_id = {access_key}\nsecret_access_key = {secret_key}\n[storage.disks.plain]\ndriver = 's3'\nbucket = {plain}\nregion = 'us-east-1'\nendpoint = {endpoint}\naccess_key_id = {access_key}\nsecret_access_key = {secret_key}\n"
    )));
    let mut builder = ProviderBuilder::new();
    builder.provider(StorageServiceProvider::new());
    let app = openrust::App::from_provider_runtime(builder.start(figment).await?)?;
    let storage = app.state().resolve::<Storage>().await?;
    let versioned = storage.disk("versioned").await?;
    let plain = storage.disk("plain").await?;
    let prefix = format!("acceptance/{}", unique_suffix());
    let object_path = StoragePath::new(format!("{prefix}/versioned.bin"))?;

    let first = versioned
        .put_stream(
            object_path.clone(),
            Some("application/octet-stream".into()),
            Box::pin(stream::iter([Ok(Bytes::from_static(b"first version"))])),
            UploadOptions::default(),
            None,
        )
        .await?;
    assert!(first.metadata.version.is_some());
    let first_version = first.metadata.version.clone().ok_or_else(|| {
        Error::custom(
            "s3_version_missing",
            "versioned bucket did not return an object version",
        )
    })?;
    assert_eq!(
        first.metadata.content_type.as_deref(),
        Some("application/octet-stream")
    );
    let old_url = versioned
        .presign_read(&object_path, Duration::from_secs(60))
        .await?;
    let expiring_url = versioned
        .presign_read(&object_path, Duration::from_secs(1))
        .await?;

    let block = Bytes::from(vec![b'x'; 1024 * 1024]);
    let large_path = StoragePath::new(format!("{prefix}/multipart.bin"))?;
    let chunks = std::iter::repeat_n(block, 12).map(Ok::<Bytes, Error>);
    let large = versioned
        .put_stream(
            large_path.clone(),
            None,
            Box::pin(stream::iter(chunks)),
            UploadOptions::default(),
            None,
        )
        .await?;
    assert_eq!(large.metadata.size, 12 * 1024 * 1024);
    let mut large_reader = versioned.open(&large_path, None).await?;
    let mut total = 0_u64;
    while let Some(chunk) = large_reader.body.next().await {
        total += chunk?.len() as u64;
    }
    assert_eq!(total, 12 * 1024 * 1024);

    let rejected_path = StoragePath::new(format!("{prefix}/rejected.bin"))?;
    let rejection_block = Bytes::from(vec![b'r'; 1024 * 1024]);
    let rejection_chunks = std::iter::repeat_n(rejection_block, 12).map(Ok::<Bytes, Error>);
    assert!(
        versioned
            .put_stream(
                rejected_path.clone(),
                None,
                Box::pin(stream::iter(rejection_chunks)),
                UploadOptions::default(),
                Some(Arc::new(RejectAfter {
                    chunks: std::sync::atomic::AtomicUsize::new(0),
                })),
            )
            .await
            .is_err()
    );
    assert!(versioned.stat(&rejected_path).await.is_err());

    let cancelled_path = StoragePath::new(format!("{prefix}/cancelled.bin"))?;
    let slow = stream::unfold((), |()| async {
        tokio::time::sleep(Duration::from_millis(35)).await;
        Some((Ok(Bytes::from(vec![b'z'; 1024 * 1024])), ()))
    });
    let upload = tokio::spawn({
        let versioned = versioned.clone();
        let cancelled_path = cancelled_path.clone();
        async move {
            versioned
                .put_stream(
                    cancelled_path,
                    None,
                    Box::pin(slow) as StorageStream,
                    UploadOptions::default(),
                    None,
                )
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(270)).await;
    upload.abort();
    let _ = upload.await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(versioned.stat(&cancelled_path).await.is_err());

    versioned
        .put_stream(
            object_path.clone(),
            Some("application/octet-stream".into()),
            Box::pin(stream::iter([Ok(Bytes::from_static(b"second version"))])),
            UploadOptions::default(),
            None,
        )
        .await?;
    let mut historical = versioned.open(&object_path, Some(&first_version)).await?;
    assert_eq!(
        historical.metadata.version.as_deref(),
        Some(first_version.as_str())
    );
    let mut historical_body = Vec::new();
    while let Some(chunk) = historical.body.next().await {
        historical_body.extend_from_slice(&chunk?);
    }
    assert_eq!(historical_body, b"first version");
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(old_url)
        .send()
        .await?;
    assert!(response.status().is_success());
    assert_eq!(response.bytes().await?.as_ref(), b"first version");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let expired = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(expiring_url)
        .send()
        .await?;
    assert!(!expired.status().is_success());

    let no_version_path = StoragePath::new(format!("{prefix}/unversioned.bin"))?;
    plain
        .put_stream(
            no_version_path.clone(),
            None,
            Box::pin(stream::iter([Ok(Bytes::from_static(b"plain bucket"))])),
            UploadOptions::default(),
            None,
        )
        .await?;
    assert!(
        plain
            .presign_read(&no_version_path, Duration::from_secs(60))
            .await
            .is_err()
    );

    for path in [
        object_path,
        large_path,
        rejected_path,
        cancelled_path,
        no_version_path,
    ] {
        let disk = if path.as_str().ends_with("unversioned.bin") {
            &plain
        } else {
            &versioned
        };
        disk.delete(&path).await?;
    }
    Ok(())
}

struct RejectAfter {
    chunks: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl UploadInspector for RejectAfter {
    async fn inspect_chunk(
        &self,
        _context: &openrust::storage::UploadContext,
        _chunk: &[u8],
    ) -> Result<()> {
        if self
            .chunks
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            == 11
        {
            Err(Error::Forbidden("验收 inspector 拒绝内容".into()))
        } else {
            Ok(())
        }
    }
}

fn unique_suffix() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
        .to_string()
}
