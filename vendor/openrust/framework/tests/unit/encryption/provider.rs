use super::*;
use crate::foundation::BootContext;
use crate::foundation::ProviderBuilder;
use crate::support::{BoxFuture, ServiceProvider};
use figment::{Figment, providers::Serialized};
use serde_json::json;

#[tokio::test]
async fn configured_provider_resolves_ordered_keys_and_rejects_invalid_history_on_first_use()
-> Result<()> {
    let old_key = "0123456789abcdef";
    let new_key = "fedcba9876543210";
    let first_history_key = "unrelated-key-00";
    let with_single_key = |key: &str| {
        ConfiguredEncrypter::from_config(crate::config::AppConfig {
            key: Some(key.to_owned()),
            cipher: "AES-128-CBC".to_owned(),
            ..crate::config::AppConfig::default()
        })
    };
    let old = with_single_key(old_key)?;
    let current_only = with_single_key(new_key)?;
    let first_history_only = with_single_key(first_history_key)?;
    let wire = old.encrypt_string(b"binary\0value\xff")?;
    for (history, valid) in [
        (
            vec![
                first_history_key.to_owned(),
                format!("base64:%{}", STANDARD.encode(old_key)),
            ],
            true,
        ),
        (
            vec![old_key.to_owned(), "secret-invalid-history".to_owned()],
            false,
        ),
    ] {
        let sources = Figment::from(Serialized::defaults(json!({
            "app": {"cipher": "aes-128-cbc", "key": new_key, "previous_keys": history},
        })));
        let mut providers = ProviderBuilder::new();
        providers.provider(EncryptionServiceProvider);
        let runtime = providers.start(sources).await?;
        let result = runtime
            .container()
            .resolve::<dyn ApplicationEncrypter>()
            .await;
        if valid {
            let service = result?;
            assert_eq!(service.decrypt_string(&wire)?, b"binary\0value\xff");
            let new_wire = service.encrypt_string(b"new")?;
            assert!(old.decrypt_string(&new_wire).is_err());
            assert!(first_history_only.decrypt_string(&new_wire).is_err());
            assert_eq!(current_only.decrypt_string(&new_wire)?, b"new");
            assert_eq!(service.decrypt_string(&new_wire)?, b"new");
            let same = runtime
                .container()
                .resolve::<dyn ApplicationEncrypter>()
                .await?;
            assert!(Arc::ptr_eq(&service, &same));
        } else {
            assert!(result.is_err());
            if let Err(error) = result {
                assert!(!error.to_string().contains("secret-invalid-history"));
            }
        }
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;
    }
    Ok(())
}

struct Rotate;
impl ServiceProvider for Rotate {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.overlay("security.encryption", json!({"active_key_id": "new"})) })
    }
}

#[tokio::test]
async fn provider_uses_overlay_key_and_decrypts_old_key() -> Result<()> {
    let old = STANDARD.encode([1u8; 32]);
    let new = STANDARD.encode([2u8; 32]);
    let prior = Encrypter::new(EncryptionConfig::new(
        "old",
        BTreeMap::from([("old".into(), old.clone())]),
    ))?;
    let ciphertext = prior.encrypt(b"message")?;
    let sources = Figment::from(Serialized::defaults(
        json!({"security":{"encryption":{"active_key_id":"old","keys":{"old":old,"new":new}}}}),
    ));
    let mut providers = ProviderBuilder::new();
    providers
        .provider(Rotate)
        .provider(EncryptionServiceProvider);
    let runtime = providers.start(sources).await?;
    let service = runtime.container().resolve::<Encrypter>().await?;
    assert!(service.encrypt(b"next")?.starts_with("or1.new."));
    assert_eq!(service.decrypt(&ciphertext)?, b"message");
    let same = runtime.container().resolve::<Encrypter>().await?;
    assert!(Arc::ptr_eq(&service, &same));
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn provider_rejects_missing_and_invalid_keys_without_secret_output() -> Result<()> {
    for source in [
        json!({}),
        json!({"security":{"encryption":{"active_key_id":"main","keys":{"main":"secret-sentinel"}}}}),
    ] {
        let mut providers = ProviderBuilder::new();
        providers.provider(EncryptionServiceProvider);
        let runtime = providers
            .start(Figment::from(Serialized::defaults(source)))
            .await?;
        let result = runtime.container().resolve::<Encrypter>().await;
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
        }
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;
    }
    Ok(())
}
