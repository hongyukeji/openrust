//! 固定公开事件名，并核对类与具名监听使用同一次派发。

use std::sync::Arc;

use super::{
    RecoveryCodeReplaced, RecoveryCodesGenerated, TwoFactorAuthenticationChallenged,
    TwoFactorAuthenticationConfirmed, TwoFactorAuthenticationDisabled,
    TwoFactorAuthenticationEnabled, TwoFactorAuthenticationFailed,
    ValidTwoFactorAuthenticationCodeProvided,
};
use crate::{
    Result,
    auth::GenericUser,
    events::{Event, Events, EventsServiceProvider},
    foundation::ProviderBuilder,
    support::DataValue,
};

#[test]
fn two_factor_events_have_distinct_responsibility_names() {
    assert_eq!(
        [
            TwoFactorAuthenticationEnabled::NAME,
            TwoFactorAuthenticationConfirmed::NAME,
            TwoFactorAuthenticationDisabled::NAME,
            RecoveryCodesGenerated::NAME,
            RecoveryCodeReplaced::NAME,
            TwoFactorAuthenticationChallenged::NAME,
            TwoFactorAuthenticationFailed::NAME,
            ValidTwoFactorAuthenticationCodeProvided::NAME,
        ],
        [
            r"OpenRust\Auth\TwoFactor\Events\TwoFactorAuthenticationEnabled",
            r"OpenRust\Auth\TwoFactor\Events\TwoFactorAuthenticationConfirmed",
            r"OpenRust\Auth\TwoFactor\Events\TwoFactorAuthenticationDisabled",
            r"OpenRust\Auth\TwoFactor\Events\RecoveryCodesGenerated",
            r"OpenRust\Auth\TwoFactor\Events\RecoveryCodeReplaced",
            r"OpenRust\Auth\TwoFactor\Events\TwoFactorAuthenticationChallenged",
            r"OpenRust\Auth\TwoFactor\Events\TwoFactorAuthenticationFailed",
            r"OpenRust\Auth\TwoFactor\Events\ValidTwoFactorAuthenticationCodeProvided",
        ],
    );
}

#[tokio::test]
async fn typed_exact_and_wildcard_two_factor_subscriptions_share_one_dispatch() -> Result<()> {
    let mut listeners = EventsServiceProvider::new();
    listeners.listen_named_reply(r"OpenRust\Auth\TwoFactor\Events\*", |_, context| {
        Box::pin(async move {
            assert_eq!(context.event_name(), TwoFactorAuthenticationEnabled::NAME);
            Ok(DataValue::from("wildcard"))
        })
    });
    listeners.listen_reply::<TwoFactorAuthenticationEnabled, _>(|event, context| {
        Box::pin(async move {
            assert_eq!(event.user.auth_identifier()?, serde_json::json!(42));
            assert_eq!(context.event_name(), TwoFactorAuthenticationEnabled::NAME);
            Ok(DataValue::from("typed"))
        })
    });
    listeners.listen_named_reply(
        r"OpenRust\Auth\TwoFactor\Events\TwoFactorAuthenticationEnabled",
        |payload, _| {
            Box::pin(async move {
                assert!(
                    payload
                        .downcast_ref::<TwoFactorAuthenticationEnabled>()
                        .is_some()
                );
                Ok(DataValue::from("exact"))
            })
        },
    );
    listeners.listen_named_reply(r"Laravel\Fortify\Events\*", |_, _| {
        Box::pin(async { Ok(DataValue::from("old namespace")) })
    });
    listeners.listen_named_reply(r"OpenRust\Auth\Events\*", |_, _| {
        Box::pin(async { Ok(DataValue::from("general auth namespace")) })
    });
    let mut builder = ProviderBuilder::new();
    builder.provider(listeners);
    let runtime = builder.start(figment::Figment::new()).await?;
    let dispatcher = runtime.container().resolve::<Events>().await?;
    let user = GenericUser::new(serde_json::Map::from_iter([("id".into(), 42.into())]));
    let responses = dispatcher
        .dispatch(&TwoFactorAuthenticationEnabled {
            user: Arc::new(user),
        })
        .await?
        .ok_or_else(|| crate::Error::custom("test_event_response_missing", "事件未立即返回响应"))?;
    let responses = responses
        .iter()
        .map(DataValue::to_json)
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(
        responses,
        ["typed", "exact", "wildcard"].map(serde_json::Value::from)
    );
    Ok(())
}
