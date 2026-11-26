use openrust::{
    auth::{AuthFuture, AuthIdentifier, AuthResult, passwords::PasswordBrokerManager},
    contracts::auth::{Authenticatable, CanResetPassword},
    foundation::Application,
    support::facades::{App, Facade as _, Password},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
struct Person(&'static str);
struct Other(Person);
macro_rules! identity {
    ($kind:ty,$email:expr) => {
        impl Authenticatable for $kind {
            fn get_auth_identifier_name(&self) -> &str {
                "email"
            }
            fn get_auth_identifier(&self) -> AuthIdentifier {
                AuthIdentifier::String(($email)(self).into())
            }
            fn get_auth_password_name(&self) -> &str {
                "password"
            }
            fn get_auth_password(&self) -> Option<&str> {
                None
            }
            fn get_remember_token(&self) -> Option<&str> {
                None
            }
            fn set_remember_token(&mut self, _: Option<String>) {}
            fn get_remember_token_name(&self) -> &str {
                "remember_token"
            }
        }
        impl CanResetPassword for $kind {
            fn get_email_for_password_reset(&self) -> &str {
                ($email)(self)
            }
            fn send_password_reset_notification<'a>(
                &'a self,
                _: &'a str,
            ) -> AuthFuture<'a, AuthResult<()>> {
                Box::pin(async { Ok(()) })
            }
        }
    };
}
identity!(Person, |user: &Person| user.0);
identity!(Other, |user: &Other| user.0.0);

#[tokio::test]
async fn facade_binding_is_scoped_composable_and_applies_to_cached_brokers() {
    let mut app=Application::configure(std::env::temp_dir()).with_config(|_,_|Ok([
        ("app".into(),json!({"key":"a".repeat(32)})),
        ("database".into(),json!({"default":"test","connections":{"test":{"driver":"sqlite","database":":memory:"}}})),
        ("auth".into(),json!({"defaults":{"passwords":"users"},"providers":{"users":{"driver":"database","table":"users"},"delegated":{"driver":"binding-proxy"}},"passwords":{"users":{"driver":"cache","store":"array","provider":"users","expire":60,"throttle":0},"delegated":{"driver":"cache","store":"array","provider":"delegated","expire":60,"throttle":0}}})),
        ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
        ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
    ].into())).create().unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        let manager = app.make::<PasswordBrokerManager>().await.unwrap();
        let broker = manager.broker(None).unwrap();
        let user = Person("shared@example.test");
        let raw = broker.create_token(&user).await.unwrap();
        assert!(broker.get_repository().exists(&user, &raw).await.unwrap());
        let revision = Arc::new(AtomicUsize::new(0));
        let value = revision.clone();
        Password::bind_user_tokens_using::<Person>(None, "revision", move |_| {
            Ok(value.load(Ordering::SeqCst).to_string())
        })
        .unwrap();
        assert!(!broker.get_repository().exists(&user, &raw).await.unwrap());
        let token = broker.create_token(&user).await.unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(broker.get_repository().exists(&user, &token).await.unwrap());
        let value = revision.clone();
        // 替换同名规则不增加重复绑定，已缓存的Broker立即读取同一注册表。
        Password::bind_user_tokens_using::<Person>(None, "revision", move |_| {
            Ok(value.load(Ordering::SeqCst).to_string())
        })
        .unwrap();
        assert!(broker.get_repository().exists(&user, &token).await.unwrap());
        revision.store(1, Ordering::SeqCst);
        assert!(!broker.get_repository().exists(&user, &token).await.unwrap());
        Password::bind_user_tokens_using::<Person>(None, "long-material", |_| {
            Ok("绑定".repeat(2000))
        })
        .unwrap();
        let token = broker.create_token(&user).await.unwrap();
        assert!(
            broker.get_repository().exists(&user, &token).await.unwrap(),
            "长绑定不能触发bcrypt输入截断"
        );
        let deny = Arc::new(AtomicBool::new(false));
        let check = deny.clone();
        Password::bind_user_tokens_using::<Person>(None, "additional-rule", move |_| {
            if check.load(Ordering::SeqCst) {
                return Err(openrust::auth::AuthError::Configuration("测试拒绝"));
            }
            Ok("rule".into())
        })
        .unwrap();
        let token = broker.create_token(&user).await.unwrap();
        deny.store(true, Ordering::SeqCst);
        assert!(broker.get_repository().exists(&user, &token).await.is_err());
        assert!(
            broker.create_token(&user).await.is_err(),
            "错误不能回退为原生无绑定签发"
        );
        let other = Other(Person("shared@example.test"));
        let untouched = broker.create_token(&other).await.unwrap();
        assert!(
            broker
                .get_repository()
                .exists(&other, &untouched)
                .await
                .unwrap(),
            "不匹配的用户类型保留原生行为"
        );
        assert!(Password::bind_user_tokens_using::<Person>(None, "", |_| Ok("x".into())).is_err());
        deny.store(false, Ordering::SeqCst);
        Password::bind_user_tokens_using::<Person>(None, "empty-value", |_| Ok(String::new()))
            .unwrap();
        assert!(broker.create_token(&user).await.is_err());
        app.make::<openrust::auth::AuthManager>()
            .await
            .unwrap()
            .provider("binding-proxy", |_, _| Ok(Arc::new(DelegatedProvider)));
        Password::bind_user_tokens_using::<Delegated>(Some("delegated"), "source", |_| {
            Ok("owner".into())
        })
        .unwrap();
        let delegated = manager.broker(Some("delegated")).unwrap();
        let credentials = openrust::auth::Credentials::new(
            std::iter::empty::<(String, openrust::database::Value)>(),
            "",
        );
        assert!(
            delegated.get_user(&credentials).await.is_err(),
            "包装类型不能静默绕过选中模型的绑定"
        );
        Password::bind_user_tokens_using::<Other>(Some("delegated"), "adapter", |user| {
            Ok(user.0.0.into())
        })
        .unwrap();
        let original = delegated.get_user(&credentials).await.unwrap().unwrap();
        let capability = original.as_password_reset().unwrap();
        let token = delegated.create_token(capability).await.unwrap();
        assert!(
            delegated
                .get_repository()
                .exists(capability, &token)
                .await
                .unwrap()
        );
    })
    .await;
}

struct Delegated(Other);
impl Authenticatable for Delegated {
    fn get_auth_identifier_name(&self) -> &str {
        "email"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.0.get_auth_identifier()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn as_password_reset(&self) -> Option<&dyn CanResetPassword> {
        Some(&self.0)
    }
}
struct DelegatedProvider;
impl openrust::contracts::auth::UserProvider for DelegatedProvider {
    fn retrieve_by_id<'a>(
        &'a self,
        _: &'a AuthIdentifier,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async { Ok(None) })
    }
    fn retrieve_by_token<'a>(
        &'a self,
        _: &'a AuthIdentifier,
        _: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async { Ok(None) })
    }
    fn update_remember_token<'a>(
        &'a self,
        _: &'a mut dyn Authenticatable,
        _: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn retrieve_by_credentials<'a>(
        &'a self,
        _: &'a openrust::auth::Credentials,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async {
            Ok(Some(
                Box::new(Delegated(Other(Person("delegated@example.test"))))
                    as Box<dyn Authenticatable>,
            ))
        })
    }
    fn validate_credentials<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        _: &'a openrust::auth::Credentials,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a mut dyn Authenticatable,
        _: &'a openrust::auth::Credentials,
        _: bool,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async { Ok(()) })
    }
}
