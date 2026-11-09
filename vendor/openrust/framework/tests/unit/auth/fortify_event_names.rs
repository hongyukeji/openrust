//! 固定公开事件名，并核对类与具名监听使用同一次派发。

use std::sync::Arc;

use super::{actions, login};
use crate::{
    Result,
    auth::{AuthUser, GenericUser},
    events::{Event, Events, EventsServiceProvider},
    fortify::events::{
        RecoveryCodeReplaced, RecoveryCodesGenerated, TwoFactorAuthenticationChallenged,
        TwoFactorAuthenticationConfirmed, TwoFactorAuthenticationDisabled,
        TwoFactorAuthenticationEnabled, TwoFactorAuthenticationFailed,
        ValidTwoFactorAuthenticationCodeProvided,
    },
    foundation::ProviderBuilder,
    support::DataValue,
};

#[test]
fn two_factor_events_use_the_fixed_fortify_namespace() {
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
            r"OpenRust\Fortify\Events\TwoFactorAuthenticationEnabled",
            r"OpenRust\Fortify\Events\TwoFactorAuthenticationConfirmed",
            r"OpenRust\Fortify\Events\TwoFactorAuthenticationDisabled",
            r"OpenRust\Fortify\Events\RecoveryCodesGenerated",
            r"OpenRust\Fortify\Events\RecoveryCodeReplaced",
            r"OpenRust\Fortify\Events\TwoFactorAuthenticationChallenged",
            r"OpenRust\Fortify\Events\TwoFactorAuthenticationFailed",
            r"OpenRust\Fortify\Events\ValidTwoFactorAuthenticationCodeProvided",
        ],
    );
}

async fn assert_dispatch<E: Event>(event: E, user_of: fn(&E) -> &AuthUser) -> Result<()> {
    let mut listeners = EventsServiceProvider::new();
    listeners.listen_named_reply(r"OpenRust\Fortify\Events\*", move |payload, context| {
        Box::pin(async move {
            let event = payload
                .downcast_ref::<E>()
                .ok_or_else(|| crate::Error::custom("event_type_mismatch", "通配符载荷类型错误"))?;
            assert_eq!(user_of(event).auth_identifier()?, serde_json::json!(42));
            assert_eq!(context.event_name(), E::NAME);
            assert!(context.transaction().is_none());
            Ok(DataValue::from("wildcard"))
        })
    });
    listeners.listen_reply::<E, _>(move |event, context| {
        Box::pin(async move {
            assert_eq!(user_of(event).auth_identifier()?, serde_json::json!(42));
            assert_eq!(context.event_name(), E::NAME);
            assert!(context.transaction().is_none());
            Ok(DataValue::from("typed"))
        })
    });
    listeners.listen_named_reply(E::NAME, move |payload, context| {
        Box::pin(async move {
            let event = payload.downcast_ref::<E>().ok_or_else(|| {
                crate::Error::custom("event_type_mismatch", "精确名称载荷类型错误")
            })?;
            assert_eq!(user_of(event).auth_identifier()?, serde_json::json!(42));
            assert_eq!(context.event_name(), E::NAME);
            assert!(context.transaction().is_none());
            Ok(DataValue::from("exact"))
        })
    });
    let type_name = E::NAME.rsplit('\\').next().unwrap_or(E::NAME);
    for prefix in [
        r"Laravel\Fortify\Events\",
        r"OpenRust\Auth\TwoFactor\Events\",
        r"OpenRust\Auth\Events\",
    ] {
        for selector in [format!("{prefix}*"), format!("{prefix}{type_name}")] {
            listeners.listen_named_reply(selector, |_, _| {
                Box::pin(async { Ok(DataValue::from("unexpected legacy match")) })
            });
        }
    }
    let mut builder = ProviderBuilder::new();
    builder.provider(listeners);
    let runtime = builder.start(figment::Figment::new()).await?;
    let dispatcher = runtime.container().resolve::<Events>().await?;
    let responses = dispatcher
        .dispatch(&event)
        .await?
        .ok_or_else(|| crate::Error::custom("test_event_response_missing", "事件未立即返回响应"))?;
    let responses = responses
        .iter()
        .map(DataValue::to_json)
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(
        responses,
        ["typed", "exact", "wildcard"].map(serde_json::Value::from),
        "{}",
        E::NAME,
    );
    Ok(())
}

#[tokio::test]
async fn typed_exact_and_wildcard_two_factor_subscriptions_share_one_dispatch() -> Result<()> {
    let user: AuthUser = Arc::new(GenericUser::new(serde_json::Map::from_iter([(
        "id".into(),
        42.into(),
    )])));
    // 动作定义传入公开类型参数，编译时约束二者为同一类型。
    assert_dispatch::<TwoFactorAuthenticationEnabled>(
        actions::TwoFactorAuthenticationEnabled { user: user.clone() },
        |event| &event.user,
    )
    .await?;
    assert_dispatch::<TwoFactorAuthenticationConfirmed>(
        actions::TwoFactorAuthenticationConfirmed { user: user.clone() },
        |event| &event.user,
    )
    .await?;
    assert_dispatch::<TwoFactorAuthenticationDisabled>(
        actions::TwoFactorAuthenticationDisabled { user: user.clone() },
        |event| &event.user,
    )
    .await?;
    assert_dispatch::<RecoveryCodesGenerated>(
        actions::RecoveryCodesGenerated { user: user.clone() },
        |event| &event.user,
    )
    .await?;
    assert_dispatch::<RecoveryCodeReplaced>(
        actions::RecoveryCodeReplaced {
            user: user.clone(),
            code: actions::RecoveryCode::new("fixture-recovery-code".into()),
        },
        |event| {
            assert_eq!(event.code.expose(), "fixture-recovery-code");
            &event.user
        },
    )
    .await?;
    assert_dispatch::<TwoFactorAuthenticationChallenged>(
        login::TwoFactorAuthenticationChallenged { user: user.clone() },
        |event| &event.user,
    )
    .await?;
    assert_dispatch::<TwoFactorAuthenticationFailed>(
        login::TwoFactorAuthenticationFailed { user: user.clone() },
        |event| &event.user,
    )
    .await?;
    assert_dispatch::<ValidTwoFactorAuthenticationCodeProvided>(
        login::ValidTwoFactorAuthenticationCodeProvided { user },
        |event| &event.user,
    )
    .await
}
