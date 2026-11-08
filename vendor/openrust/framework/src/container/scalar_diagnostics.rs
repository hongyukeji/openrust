//! 构造弱标量的同步诊断策略与应用启动接线。

use std::backtrace::Backtrace;
use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use super::ScalarDiagnostic;
use crate::config::repository::ConfigRepository;
use crate::core::{Error, Result};
use crate::foundation::lifecycle::catch_isolated;

const WARNING: i64 = 2;

type Report = dyn Fn(ScalarDiagnostic) -> Result<()> + Send + Sync;

/// 构造弱标量的同步诊断策略。
///
/// 实参值不会交给诊断回调。回调失败会终止构造，并保留原错误。
/// 独立容器默认通过 `tracing` 记录后继续，应用启动则按配置处理。
#[derive(Clone)]
pub struct ScalarDiagnostics(Arc<Report>);

impl ScalarDiagnostics {
    /// 安装同步回调。回调在对应参数物化期间运行。
    pub fn new(report: impl Fn(ScalarDiagnostic) -> Result<()> + Send + Sync + 'static) -> Self {
        Self(Arc::new(report))
    }

    pub(super) fn report(&self, event: ScalarDiagnostic) -> Result<()> {
        (self.0)(event)
    }

    pub(crate) fn application(config: ConfigRepository) -> Self {
        Self::new(move |event| {
            if event.is_deprecation() {
                report_deprecation(&config, event);
            } else if warning_enabled(&config) {
                return Err(Error::http_with_source(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "container_scalar_warning",
                    event.to_string(),
                    event,
                ));
            }
            Ok(())
        })
    }
}

impl Default for ScalarDiagnostics {
    fn default() -> Self {
        Self::new(|event| {
            let _ = catch_isolated(|| tracing::warn!(diagnostic = ?event, "{event}"));
            Ok(())
        })
    }
}

impl ScalarDiagnostic {
    fn is_deprecation(self) -> bool {
        matches!(
            self,
            Self::FloatLosesPrecision | Self::NumericStringLosesPrecision
        )
    }
}

impl fmt::Display for ScalarDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FloatLosesPrecision => "构造实参从浮点数转换为整数时失去精度",
            Self::NumericStringLosesPrecision => "构造实参从数字字符串转换为整数时失去精度",
            Self::NanToBytes => "构造实参将 NaN 转换为字符串",
            Self::NanToBool => "构造实参将 NaN 转换为布尔值",
        })
    }
}

impl std::error::Error for ScalarDiagnostic {}

fn warning_enabled(config: &ConfigRepository) -> bool {
    // 固定 Laravel 应用启动启用全部级别。Rust 的覆盖限定在当前应用。
    let mask = config
        .get("logging.error_reporting")
        .ok()
        .flatten()
        .and_then(|value| value.as_i64())
        .unwrap_or(-1);
    mask & WARNING != 0
}

fn report_deprecation(config: &ConfigRepository, event: ScalarDiagnostic) {
    let Ok(settings) = config.get_many_with_defaults(&[
        ("app.env", Value::Null),
        ("logging.deprecations", Value::Null),
        ("logging.channels", Value::Null),
    ]) else {
        return;
    };
    let [(_, environment), (_, options), (_, channels)] = settings.as_slice() else {
        return;
    };
    if environment.as_str() == Some("testing")
        && options.get("while_testing").and_then(Value::as_bool) != Some(true)
    {
        return;
    }
    let selected = options
        .as_str()
        .or_else(|| options.get("channel").and_then(Value::as_str))
        .unwrap_or("null");
    // 当前统一日志的可用输出是 tracing。缺失或不支持的通道沿 Laravel 的日志失败路径继续。
    let dedicated = channels
        .get("deprecations")
        .filter(|channel| channel.as_object().is_some_and(|object| !object.is_empty()));
    let driver = dedicated.or_else(|| channels.get(selected)).map_or_else(
        || (selected == "tracing").then_some("tracing"),
        |channel| channel.get("driver").and_then(Value::as_str),
    );
    if driver != Some("tracing") {
        return;
    }
    let trace = options.get("trace").and_then(Value::as_bool) == Some(true);
    let _ = catch_isolated(|| {
        if trace {
            tracing::warn!(
                target: "deprecations",
                diagnostic = ?event,
                backtrace = %Backtrace::force_capture(),
                "{event}",
            );
        } else {
            tracing::warn!(target: "deprecations", diagnostic = ?event, "{event}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigSources;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::{Context, SubscriberExt as _};

    #[test]
    fn warning_uses_application_mask_even_during_boot() -> Result<()> {
        let config = ConfigRepository::new(ConfigSources::from_ordered_entries(
            std::iter::empty::<(&str, Value)>(),
        )?);
        let policy = ScalarDiagnostics::application(config.clone());
        let error = policy
            .report(ScalarDiagnostic::NanToBool)
            .err()
            .ok_or_else(|| Error::custom("expected_scalar_warning", "应用默认升级 warning"))?;
        assert_eq!(error.code(), "container_scalar_warning");
        assert_eq!(
            std::error::Error::source(&error)
                .and_then(|cause| cause.downcast_ref::<ScalarDiagnostic>())
                .copied(),
            Some(ScalarDiagnostic::NanToBool)
        );
        config.set("logging.error_reporting", json!(0))?;
        policy.report(ScalarDiagnostic::NanToBytes)?;
        config.set("logging.error_reporting", json!(2))?;
        assert!(matches!(
            policy.report(ScalarDiagnostic::NanToBytes),
            Err(error) if error.code() == "container_scalar_warning"
        ));
        ScalarDiagnostics::default().report(ScalarDiagnostic::NanToBytes)?;
        Ok(())
    }

    #[derive(Clone, Default)]
    struct Capture {
        events: Arc<AtomicUsize>,
        traced: Arc<AtomicUsize>,
    }

    impl<S: tracing::Subscriber> Layer<S> for Capture {
        fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
            if event.metadata().target() == "deprecations" {
                self.events.fetch_add(1, Ordering::Relaxed);
                if event.metadata().fields().field("backtrace").is_some() {
                    self.traced.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    #[test]
    fn deprecations_obey_exact_testing_environment_and_selected_channels() -> Result<()> {
        let config = ConfigRepository::new(ConfigSources::from_ordered_entries([
            ("app", json!({"env": "testing"})),
            (
                "logging",
                json!({
                    "deprecations": {"channel": "selected"},
                    "channels": {
                        "selected": {"driver": "tracing"},
                        "ignored": {"driver": "null"}
                    }
                }),
            ),
        ])?);
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        tracing::subscriber::with_default(subscriber, || -> Result<()> {
            let event = ScalarDiagnostic::NumericStringLosesPrecision;
            let policy = ScalarDiagnostics::application(config.clone());
            policy.report(event)?;
            assert_eq!(capture.events.load(Ordering::Relaxed), 0);
            config.set("logging.deprecations.while_testing", json!(true))?;
            policy.report(event)?;
            assert_eq!(capture.events.load(Ordering::Relaxed), 1);
            assert_eq!(capture.traced.load(Ordering::Relaxed), 0);
            config.set("logging.deprecations.trace", json!(true))?;
            policy.report(event)?;
            assert_eq!(capture.events.load(Ordering::Relaxed), 2);
            assert_eq!(capture.traced.load(Ordering::Relaxed), 1);

            config.set("logging.channels.deprecations.driver", json!("null"))?;
            policy.report(event)?;
            assert_eq!(capture.events.load(Ordering::Relaxed), 2);
            config.set("logging.channels.deprecations", Value::Null)?;
            config.set("logging.deprecations", json!("tracing"))?;
            config.set("app.env", json!("production"))?;
            policy.report(event)?;
            assert_eq!(capture.events.load(Ordering::Relaxed), 3);
            config.set("logging.deprecations", Value::Null)?;
            policy.report(event)?;
            assert_eq!(capture.events.load(Ordering::Relaxed), 3);
            Ok(())
        })
    }
    struct RegisterScalar;

    impl crate::support::ServiceProvider for RegisterScalar {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                use crate::container::{ParameterKey, ParameterOverrides, ParameterValue};
                let mut parameters = ParameterOverrides::new();
                parameters.insert(ParameterKey::name("value"), ParameterValue::new(2.5_f64));
                assert_eq!(*app.resolve_with::<i64>(parameters).await?, 2);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn provider_register_reports_deprecation_after_configuration_bootstrap() -> Result<()> {
        use crate::container::{ClassDescriptor, ConstructorParam, PhpTarget};
        let capture = Capture::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let _guard = tracing::subscriber::set_default(subscriber);
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder
            .class_descriptors([ClassDescriptor::concrete::<i64>(
                "Test/ProviderScalar",
                vec![ConstructorParam::php("value", PhpTarget::Int)],
                |args| args.value(0)?.downcast::<i64>(),
            )])
            .provider(RegisterScalar);
        let runtime = builder
            .start(ConfigSources::from_ordered_entries([
                ("app", json!({"env": "local"})),
                ("logging", json!({"deprecations": "tracing"})),
            ])?)
            .await?;
        assert_eq!(capture.events.load(Ordering::Relaxed), 1);
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(3))
            .await?;
        Ok(())
    }
}
