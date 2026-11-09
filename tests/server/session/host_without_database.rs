//! 根应用的 Session Provider 选择。

use super::Server;
use figment::{Figment, providers::Serialized};
use openrust::Result;
use openrust::config::repository::ConfigRepository;
use openrust::foundation::{ApplicationDefinition, ProviderId};
use openrust::session::SessionServiceProvider;
use serde_json::json;

fn selected_session_provider_count(
    config: &openrust::config::DatabaseConfig,
    driver: Option<&str>,
) -> Result<usize> {
    let sources = Figment::from(Serialized::defaults(json!({
        "database": {"url": &config.url},
        "session": {"driver": driver}
    })));
    let repository = ConfigRepository::new(sources);
    let defaults = Server::default_providers().resolve(&repository)?;
    let application = Server::providers(&super::Configuration::default());
    let session = ProviderId::of::<SessionServiceProvider>();
    Ok(defaults
        .iter()
        .chain(application.iter())
        .filter(|provider| provider.id() == session)
        .count())
}

#[test]
fn database_host_selects_one_session_provider_even_without_auth() -> Result<()> {
    let config = openrust::config::DatabaseConfig {
        url: Some("postgres://placeholder.invalid/not_connected".into()),
        ..openrust::config::DatabaseConfig::default()
    };
    assert_eq!(
        selected_session_provider_count(&config, Some("database"))?,
        1
    );
    Ok(())
}

#[test]
fn explicit_non_database_driver_selects_once_without_database() -> Result<()> {
    for driver in [Some("cookie"), Some("array"), Some("unknown"), None] {
        let config = openrust::config::DatabaseConfig::default();
        let count = selected_session_provider_count(&config, driver)?;
        assert_eq!(count, usize::from(driver.is_some()));
    }
    Ok(())
}
