use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;

fn encryptor(active: &str) -> crate::Result<crate::encryption::ContextualEncrypter> {
    crate::encryption::ContextualEncrypter::new(crate::config::EncryptionConfig::new(
        active,
        [
            ("old".to_owned(), STANDARD.encode([7u8; 32])),
            ("current".to_owned(), STANDARD.encode([8u8; 32])),
        ]
        .into(),
    ))
}
fn ciphertext(protection: &Protection, name: &str, value: &str) -> crate::Result<String> {
    let cookie = super::super::CookieFactory::default().make(
        name,
        value,
        0,
        super::super::CookieOptions::default(),
    )?;
    Ok(protection
        .encrypt(&cookie, &[])?
        .value()
        .unwrap_or_default()
        .to_owned())
}
#[test]
fn authenticated_name_mismatch_is_leaf_null_but_bad_tag_nulls_root() -> crate::Result<()> {
    let old = Protection::Encrypted(encryptor("old")?);
    let current = Protection::Encrypted(encryptor("current")?);
    let good = ciphertext(&old, "preferences[a]", "semi;plus+雪")?;
    let replay = ciphertext(&old, "different", "hidden")?;
    let mut tree = json!({"preferences":{"a":good,"b":replay}});
    current.decrypt(&mut tree, &[]);
    assert_eq!(tree, json!({"preferences":{"a":"semi;plus+雪","b":null}}));
    let mut tree =
        json!({"preferences":{"a":ciphertext(&old,"preferences[a]","ok")?,"b":"or1.invalid"}});
    current.decrypt(&mut tree, &[]);
    assert_eq!(tree, json!({"preferences":null}));
    assert!(ciphertext(&current, "preferences", "new")?.starts_with("or1.current."));
    Ok(())
}
#[tokio::test]
async fn name_bound_protection_uses_history_and_distinguishes_prefix_miss_from_bad_ciphertext()
-> crate::Result<()> {
    use crate::encryption::{ApplicationEncrypter, EncryptionServiceProvider};
    use crate::foundation::{ProviderBuilder, ProviderRuntime};
    use figment::{Figment, providers::Serialized};

    async fn start(key: &str, previous_keys: Vec<String>) -> crate::Result<ProviderRuntime> {
        let mut builder = ProviderBuilder::new();
        builder.provider(EncryptionServiceProvider);
        let sources = Figment::from(Serialized::defaults(json!({
            "app": {"key": key, "previous_keys": previous_keys},
        })));
        builder.start(sources).await
    }

    let previous_key = "b".repeat(32);
    let current = start(&"a".repeat(32), vec![previous_key.clone()]).await?;
    let previous = start(&previous_key, Vec::new()).await?;
    let old_protection = Protection::NameBound(
        previous
            .container()
            .resolve::<dyn ApplicationEncrypter>()
            .await?,
    );
    let protection = Protection::NameBound(
        current
            .container()
            .resolve::<dyn ApplicationEncrypter>()
            .await?,
    );
    let valid = ciphertext(&old_protection, "group[a]", "valid")?;
    let foreign = ciphertext(&old_protection, "other", "hidden")?;
    let mut tree = json!({"group":{"a":valid,"b":foreign},"clear":"unchanged"});
    protection.decrypt(&mut tree, &["clear".to_owned()]);
    assert_eq!(
        tree,
        json!({"group":{"a":"valid","b":null},"clear":"unchanged"})
    );
    let valid = ciphertext(&old_protection, "group[a]", "valid")?;
    let mut tree = json!({"group":{"a":valid,"b":"bad ciphertext"}});
    protection.decrypt(&mut tree, &[]);
    assert_eq!(tree, json!({"group":null}));
    for runtime in [current, previous] {
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
            .await?;
    }
    Ok(())
}

#[test]
fn except_and_authenticated_invalid_payload_preserve_source_classification() -> crate::Result<()> {
    let encryptor = encryptor("current")?;
    let invalid = encryptor.encrypt_with_context(b"not a payload", COOKIE_DOMAIN)?;
    let protection = Protection::Encrypted(encryptor);
    let mut tree =
        json!({"group":{"a":invalid,"b":ciphertext(&protection,"group[b]","ok")?},"plain":"clear"});
    protection.decrypt(&mut tree, &["plain".to_owned()]);
    assert_eq!(tree, json!({"group":{"a":null,"b":"ok"},"plain":"clear"}));
    Ok(())
}
