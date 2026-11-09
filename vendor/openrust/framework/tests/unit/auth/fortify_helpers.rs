pub(crate) fn test_field_encrypter(
    key: &str,
) -> crate::Result<std::sync::Arc<dyn crate::encryption::ApplicationEncrypter>> {
    Ok(std::sync::Arc::new(
        crate::encryption::ConfiguredEncrypter::from_config(crate::config::AppConfig {
            key: Some(format!("base64:{key}")),
            ..crate::config::AppConfig::default()
        })?,
    ))
}
