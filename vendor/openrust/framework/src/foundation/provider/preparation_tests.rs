//! 外部包装配不能改变失败边界、默认槽位或首个注册回调可见的绑定。

use super::*;
use crate::core::Error;
use crate::foundation::RegisterContext;
use crate::support::DefaultProviders;
use figment::Figment;
use std::sync::Mutex;
use tokio::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<&'static str>>>);
impl Trace {
    fn push(&self, name: &'static str) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(name);
    }
    fn values(&self) -> Vec<&'static str> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
struct Probe<const N: u8>(Trace, &'static str);
impl<const N: u8> ServiceProvider for Probe<N> {
    fn register<'a>(&'a self, _: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.push(self.1);
            Ok(())
        })
    }
}
struct Plan<const N: u8> {
    trace: Trace,
    name: &'static str,
    fail: bool,
}
impl<const N: u8> ProviderRegistration for Plan<N> {
    fn prepare(
        self: Box<Self>,
        context: &mut ProviderPreparation<'_>,
    ) -> Result<Vec<Box<dyn ServiceProvider>>> {
        if self.fail {
            return Err(Error::custom("preparation_failed", "装配失败"));
        }
        context.singleton::<u32, _>(|_| Box::pin(async { Ok(Arc::new(41)) }));
        context.append_provider(Box::new(Probe::<9>(self.trace.clone(), "tail")));
        Ok(vec![Box::new(Probe::<N>(self.trace.clone(), self.name))])
    }
}
async fn finish(runtime: super::super::ProviderRuntime) -> Result<()> {
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn invalid_registration_fails_before_any_user_callback() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(Probe::<1>(trace.clone(), "must-not-run"));
    builder.registration(Plan::<2> {
        trace: trace.clone(),
        name: "invalid",
        fail: true,
    });
    let error = match builder.start(Figment::new()).await {
        Ok(runtime) => {
            finish(runtime).await?;
            return Err(Error::custom("test_expected_failure", "应在注册前失败"));
        }
        Err(error) => error,
    };
    assert_eq!(error.code(), "preparation_failed");
    assert_eq!(trace.values(), [] as [&str; 0]);
    Ok(())
}

struct ReadSeed(Trace);
impl ServiceProvider for ReadSeed {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            assert_eq!(*context.resolve::<u32>().await?, 41);
            self.0.push("first-sees-seed");
            Ok(())
        })
    }
}
#[tokio::test]
async fn prepared_binding_is_visible_before_slot_and_children_remain_at_tail() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(ReadSeed(trace.clone()));
    builder.registration(Plan::<2> {
        trace: trace.clone(),
        name: "slot",
        fail: false,
    });
    builder.provider(Probe::<3>(trace.clone(), "manual-after"));
    let runtime = builder.start(Figment::new()).await?;
    assert_eq!(
        trace.values(),
        ["first-sees-seed", "slot", "manual-after", "tail"]
    );
    finish(runtime).await
}

#[tokio::test]
async fn replacing_registration_before_defaults_keeps_defaults_at_original_boundary() -> Result<()>
{
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.registration(Plan::<2> {
        trace: trace.clone(),
        name: "old",
        fail: false,
    });
    builder.default_providers(DefaultProviders::empty().merge(vec![Box::new(Probe::<4>(
        trace.clone(),
        "default",
    ))
        as Box<dyn ServiceProvider>]));
    builder.provider(Probe::<3>(trace.clone(), "manual-after"));
    builder.registration(Plan::<2> {
        trace: trace.clone(),
        name: "replacement",
        fail: false,
    });
    let runtime = builder.start(Figment::new()).await?;
    assert_eq!(
        trace.values(),
        ["default", "manual-after", "replacement", "tail"]
    );
    finish(runtime).await
}

#[tokio::test]
async fn removing_provider_before_defaults_keeps_slot_in_bounds() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(Probe::<1>(trace.clone(), "removed"));
    builder.default_providers(DefaultProviders::empty().merge(vec![Box::new(Probe::<4>(
        trace.clone(),
        "default",
    ))
        as Box<dyn ServiceProvider>]));
    builder.remove_provider(ProviderId::of::<Probe<1>>());
    let runtime = builder.start(Figment::new()).await?;
    assert_eq!(trace.values(), ["default"]);
    finish(runtime).await
}

struct FallbackPlan(Trace);
impl ProviderRegistration for FallbackPlan {
    fn prepare(
        self: Box<Self>,
        context: &mut ProviderPreparation<'_>,
    ) -> Result<Vec<Box<dyn ServiceProvider>>> {
        context.prepend_provider_unless_present(
            Box::new(Probe::<5>(self.0.clone(), "fallback")),
            ProviderId::of::<Probe<6>>(),
        );
        Ok(Vec::new())
    }
}
struct ActualPlan(Trace);
impl ProviderRegistration for ActualPlan {
    fn prepare(
        self: Box<Self>,
        context: &mut ProviderPreparation<'_>,
    ) -> Result<Vec<Box<dyn ServiceProvider>>> {
        context.append_provider(Box::new(Probe::<6>(self.0.clone(), "real-tail")));
        Ok(Vec::new())
    }
}
#[tokio::test]
async fn conditional_fallback_observes_later_registration_tail_and_explicit_removal() -> Result<()>
{
    for removed in [false, true] {
        let trace = Trace::default();
        let mut builder = ProviderBuilder::new();
        builder.registration(FallbackPlan(trace.clone()));
        builder.registration(ActualPlan(trace.clone()));
        if removed {
            builder.remove_provider(ProviderId::of::<Probe<6>>());
        }
        let runtime = builder.start(Figment::new()).await?;
        assert_eq!(
            trace.values(),
            if removed { vec![] } else { vec!["real-tail"] }
        );
        finish(runtime).await?;
    }
    Ok(())
}

#[tokio::test]
async fn conditional_fallback_observes_bootstrap_provider() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.registration(FallbackPlan(trace.clone()));
    builder.bootstrap_provider(Probe::<6>(trace.clone(), "real-bootstrap"));
    let runtime = builder.start(Figment::new()).await?;
    assert_eq!(trace.values(), ["real-bootstrap"]);
    finish(runtime).await
}

struct CandidateOnly;
impl ServiceProvider for CandidateOnly {
    fn possible_nested_providers(&self) -> Vec<ProviderId> {
        vec![ProviderId::of::<Probe<6>>()]
    }
}
#[tokio::test]
async fn missing_or_merely_possible_nested_identity_keeps_conditional_fallback() -> Result<()> {
    for candidate in [false, true] {
        let trace = Trace::default();
        let mut builder = ProviderBuilder::new();
        builder.registration(FallbackPlan(trace.clone()));
        if candidate {
            builder.provider(CandidateOnly);
        }
        let runtime = builder.start(Figment::new()).await?;
        assert_eq!(trace.values(), ["fallback"]);
        finish(runtime).await?;
    }
    Ok(())
}
