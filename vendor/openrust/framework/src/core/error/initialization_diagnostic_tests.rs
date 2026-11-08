use super::*;
use std::error::Error as StdError;

const SECRET: &str = "smtp://credential_user:credential_password@example.invalid/private-mail-body?token=diagnostic_secret_token";

struct Mailer;
struct NotificationService;
struct RepeatedDependency;
struct OutermostService;

fn initialization<T: 'static>(source: Error) -> Error {
    Error::service_initialization::<T>(source)
}

fn assert_redacted(rendered: &str) {
    for secret in [
        "credential_user",
        "credential_password",
        "private-mail-body",
        "diagnostic_secret_token",
        "custom_secret_code",
        "custom_secret_message",
    ] {
        assert!(!rendered.contains(secret), "诊断输出泄露了敏感哨兵");
    }
}

#[test]
fn static_configuration_diagnostic_retains_private_source_chain() {
    let config = ConfigError::diagnostic_with_source(
        "mail.smtp_host",
        "required_for_smtp",
        std::io::Error::other(SECRET),
    );
    assert_redacted(&config.to_string());
    assert_redacted(&format!("{config:?}"));
    let error = initialization::<Mailer>(Error::Config(config));
    let display = error.to_string();
    assert!(display.contains("Mailer"));
    assert!(display.contains("mail.smtp_host"));
    assert!(display.contains("required_for_smtp"));
    assert_redacted(&display);
    assert_redacted(&format!("{error:?}"));

    // 诊断格式化隐藏源值，但内部报告仍能按具体错误类型取得原始来源。
    let mut current: Option<&(dyn StdError + 'static)> = Some(&error);
    let mut retained = false;
    while let Some(source) = current {
        if let Some(source) = source.downcast_ref::<std::io::Error>() {
            retained |= source.to_string() == SECRET;
        }
        current = source.source();
    }
    assert!(retained, "原始错误来源必须保留在 StdError 链中");
}

#[test]
fn nested_initialization_identifies_dependency_and_configuration_key() {
    let error = initialization::<NotificationService>(initialization::<Mailer>(Error::Config(
        ConfigError::diagnostic("mail.smtp_host", "required_for_smtp"),
    )));
    let display = error.to_string();
    let parent = display.find("NotificationService");
    let child = display.find("Mailer");
    assert!(parent.is_some() && child.is_some());
    assert!(parent < child, "诊断应按父服务到依赖服务的顺序输出");
    assert!(display.contains("mail.smtp_host"));
    assert!(display.contains("required_for_smtp"));
}

#[test]
fn arbitrary_source_messages_and_custom_codes_are_not_initialization_diagnostics() {
    let sources = [
        Error::Config(ConfigError::Invalid(SECRET.to_owned())),
        Error::internal(std::io::Error::other(SECRET)),
        Error::custom("custom_secret_code", "custom_secret_message"),
    ];
    for source in sources {
        let error = initialization::<Mailer>(source);
        assert_redacted(&error.to_string());
        assert_redacted(&format!("{error:?}"));
    }
}

#[test]
fn deep_dependency_diagnostic_is_bounded_without_losing_outer_service() {
    let mut error = Error::Config(ConfigError::diagnostic(
        "mail.smtp_host",
        "required_for_smtp",
    ));
    for _ in 0..128 {
        error = initialization::<RepeatedDependency>(error);
    }
    error = initialization::<OutermostService>(error);
    let display = error.to_string();
    assert!(display.contains("OutermostService"));
    assert!(display.len() < 4096, "深依赖链诊断必须限制输出长度");
    assert!(
        display.matches("RepeatedDependency").count() < 128,
        "达到诊断深度上限后必须截断依赖链"
    );
    assert!(format!("{error:?}").len() < 4096);
}

#[test]
fn initialization_http_response_keeps_status_and_code_without_private_details() -> Result<()> {
    let error = initialization::<Mailer>(Error::Config(ConfigError::diagnostic_with_source(
        "mail.smtp_host",
        "required_for_smtp",
        std::io::Error::other(SECRET),
    )));
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error.code(), "service_initialization_failed");
    let response = error.to_api_response();
    assert_eq!(response.code, "service_initialization_failed");
    assert_eq!(response.message, "internal server error");
    assert!(!response.success);
    let serialized = serde_json::to_string(&response).map_err(Error::internal)?;
    assert_redacted(&serialized);
    assert!(!serialized.contains("mail.smtp_host"));
    assert!(!serialized.contains("required_for_smtp"));
    assert!(!serialized.contains("Mailer"));
    assert_eq!(
        response.into_response().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    Ok(())
}

#[test]
fn long_service_identity_is_bounded_in_top_level_and_dependency_diagnostics() {
    const PREFIX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const NAME: &str = concat!(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "\n名称",
    );

    let service = ServiceIdentity::from_type_name(NAME);
    assert_eq!(
        ServiceIdentity::from_type_name("worker\n任务").to_string(),
        "worker\\n任务"
    );
    assert_eq!(service.name(), NAME);
    let rendered = service.to_string();
    assert!(rendered.starts_with(PREFIX));
    assert!(rendered.ends_with("…"));
    assert!(!rendered.contains('\n'));
    assert!(rendered.len() <= 128);

    let dependency = Error::ServiceInitialization {
        service,
        source: Arc::new(Error::internal(std::io::Error::other(SECRET))),
    };
    for rendered in [dependency.to_string(), format!("{dependency:?}")] {
        assert!(rendered.len() < 512);
        assert!(!rendered.contains('\n'));
        assert_redacted(&rendered);
    }

    let mut error = dependency;
    for _ in 0..16 {
        error = Error::ServiceInitialization {
            service,
            source: Arc::new(error),
        };
    }
    for rendered in [error.to_string(), format!("{error:?}")] {
        assert!(rendered.len() < 4096);
        assert!(!rendered.contains(NAME));
        assert!(rendered.contains("cause_chain_truncated"));
        assert_redacted(&rendered);
    }
}
