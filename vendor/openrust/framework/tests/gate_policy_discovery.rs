#![allow(non_snake_case, reason = "授权公开接口保留原名")]
use openrust::{
    auth::{
        AuthFuture, AuthResult,
        access::{Argument, GateRegistry, Policy, PolicyTarget, Response},
    },
    contracts::{
        auth::{Authenticatable, access::Gate as GateContract},
        container::Container as _,
        foundation::Application as _,
    },
    foundation::Application,
    support::facades::Gate,
};
use serde_json::{Value, json};
use std::{
    any::{TypeId, type_name},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

mod app {
    pub mod models {
        pub struct Article;
        pub struct Unmapped;
        pub mod nested {
            pub struct Entry;
        }
        pub mod policies {
            pub mod nested {
                pub struct EntryPolicy;
            }
        }
    }
    pub mod policies {
        pub struct ArticlePolicy {
            pub sequence: usize,
            pub dependency: Arc<usize>,
        }
        pub mod nested {
            pub struct EntryPolicy;
        }
        use std::sync::Arc;
    }
    pub mod alternate {
        pub struct ArticlePolicy;
    }
}
struct BrokenPolicy;
macro_rules! policy {
    ($model:ty, $policy:ty, $allowed:expr) => {
        impl Policy<$model> for $policy {
            fn has_ability(&self, ability: &str) -> bool {
                ability == "view"
            }
            fn allows_guests(&self, _: &str) -> bool {
                true
            }
            fn evaluate<'a>(
                &'a self,
                _: &'a str,
                _: Option<&'a dyn Authenticatable>,
                _: Option<&'a $model>,
                _: &'a [&'a Argument],
            ) -> AuthFuture<'a, AuthResult<Option<Response>>> {
                Box::pin(async { Ok(Some(Response::from($allowed))) })
            }
        }
    };
}
policy!(app::models::Article, app::policies::ArticlePolicy, true);
policy!(app::models::Article, app::alternate::ArticlePolicy, false);
policy!(
    app::models::nested::Entry,
    app::policies::nested::EntryPolicy,
    true
);
policy!(
    app::models::nested::Entry,
    app::models::policies::nested::EntryPolicy,
    false
);
policy!(app::models::Article, BrokenPolicy, true);

async fn fixture() -> (Application, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let constructed = count.clone();
    let mut app = Application::configure(std::env::temp_dir())
        .booted(move |app| {
            let container = app.container();
            container.instance(Arc::new(41usize))?;
            container.bind::<app::policies::ArticlePolicy, _>(move |container| {
                Ok(Arc::new(app::policies::ArticlePolicy {
                    sequence: constructed.fetch_add(1, Ordering::Relaxed),
                    dependency: container.make::<usize>()?,
                }))
            })?;
            container.bind::<app::alternate::ArticlePolicy, _>(|_| {
                Ok(Arc::new(app::alternate::ArticlePolicy))
            })?;
            container.bind::<app::policies::nested::EntryPolicy, _>(|_| {
                Ok(Arc::new(app::policies::nested::EntryPolicy))
            })?;
            container.bind::<app::models::policies::nested::EntryPolicy, _>(|_| {
                Ok(Arc::new(app::models::policies::nested::EntryPolicy))
            })?;
            Gate::discoverPolicy::<app::models::Article, app::policies::ArticlePolicy>()?;
            Gate::discoverPolicy::<app::models::Article, app::alternate::ArticlePolicy>()?;
            Gate::discoverPolicy::<app::models::nested::Entry, app::policies::nested::EntryPolicy>(
            )?;
            Gate::discoverPolicy::<
                app::models::nested::Entry,
                app::models::policies::nested::EntryPolicy,
            >()?;
            assert!(Gate::policies()?.is_empty());
            let native = Gate::resolvePolicy::<app::policies::ArticlePolicy>()?;
            assert_eq!(*native.dependency, 41);
            let policy = Gate::getPolicyFor(&app::models::Article)?.unwrap();
            assert!(policy.downcast::<app::policies::ArticlePolicy>().is_some());
            Gate::guessPolicyNamesUsing(|_| {
                vec![type_name::<app::policies::ArticlePolicy>().into()]
            })?;
            assert!(Gate::getPolicyFor(&PolicyTarget::of::<app::models::Article>())?.is_some());
            // 替换为空后不继续默认发现。
            Gate::guessPolicyNamesUsing(|_| Vec::new())?;
            assert!(Gate::getPolicyFor(&app::models::Article)?.is_none());
            Ok(())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    (app, count)
}

#[tokio::test]
async fn policy_resolution_uses_real_container_bindings_and_typed_public_entries() {
    let (app, count) = fixture().await;
    let registry = app.container().make::<GateRegistry>().unwrap();
    registry.guessPolicyNamesUsing(|_| vec![type_name::<app::policies::ArticlePolicy>().into()]);
    let scoped = registry.forUser(None);
    let contract: &dyn GateContract = &scoped;
    let handle = contract
        .getPolicyFor(&app::models::Article)
        .unwrap()
        .unwrap();
    assert_eq!(handle.name(), type_name::<app::policies::ArticlePolicy>());
    assert!(handle.downcast::<app::alternate::ArticlePolicy>().is_none());
    let one = handle.downcast::<app::policies::ArticlePolicy>().unwrap();
    let two = scoped
        .resolvePolicy::<app::policies::ArticlePolicy>()
        .unwrap();
    assert!(!Arc::ptr_eq(&one, &two));
    assert_ne!(one.sequence, two.sequence);
    assert!(Arc::ptr_eq(&one.dependency, &two.dependency));
    assert!(
        scoped
            .allows("view", &[&app::models::Article])
            .await
            .unwrap()
    );
    assert!(
        scoped
            .allows("view", &[&PolicyTarget::of::<app::models::Article>()])
            .await
            .unwrap()
    );
    assert!(count.load(Ordering::Relaxed) >= 6);
    // 容器生命周期由原绑定决定，不由发现机制另建缓存。
    let shared = Arc::new(app::policies::ArticlePolicy {
        sequence: 100,
        dependency: one.dependency.clone(),
    });
    app.container().instance(shared.clone()).unwrap();
    assert!(Arc::ptr_eq(
        &shared,
        &registry
            .resolvePolicy::<app::policies::ArticlePolicy>()
            .unwrap()
    ));
    assert!(Arc::ptr_eq(
        &shared,
        &scoped
            .getPolicyFor(&app::models::Article)
            .unwrap()
            .unwrap()
            .downcast()
            .unwrap()
    ));
    assert!(scoped.policies().is_empty());
}

#[tokio::test]
async fn policy_discovery_matches_fixed_php_precedence_and_missing_candidates() {
    let (app, _) = fixture().await;
    let registry = app.container().make::<GateRegistry>().unwrap();
    // 通过一个未替换guess回调的新应用验收默认约定。
    let mut default_app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    default_app.boot().await.unwrap();
    let default_registry = default_app.container().make::<GateRegistry>().unwrap();
    default_app
        .container()
        .bind::<app::policies::ArticlePolicy, _>(|_| {
            Ok(Arc::new(app::policies::ArticlePolicy {
                sequence: 0,
                dependency: Arc::new(0),
            }))
        })
        .unwrap();
    default_app
        .container()
        .bind::<app::policies::nested::EntryPolicy, _>(|_| {
            Ok(Arc::new(app::policies::nested::EntryPolicy))
        })
        .unwrap();
    default_app
        .container()
        .bind::<app::models::policies::nested::EntryPolicy, _>(|_| {
            Ok(Arc::new(app::models::policies::nested::EntryPolicy))
        })
        .unwrap();
    default_registry
        .discoverPolicy::<app::models::Article, app::policies::ArticlePolicy>()
        .discoverPolicy::<app::models::nested::Entry, app::policies::nested::EntryPolicy>()
        .discoverPolicy::<app::models::nested::Entry, app::models::policies::nested::EntryPolicy>();
    let mut observed = serde_json::Map::new();
    observed.insert(
        "default".into(),
        json!(
            default_registry
                .getPolicyFor(&app::models::Article)
                .unwrap()
                .unwrap()
                .name()
                .contains("app::policies::ArticlePolicy")
        ),
    );
    observed.insert(
        "nested".into(),
        json!(
            default_registry
                .getPolicyFor(&app::models::nested::Entry)
                .unwrap()
                .unwrap()
                .name()
                .contains("models::policies::nested::EntryPolicy")
        ),
    );
    observed.insert(
        "unmapped".into(),
        json!(
            default_registry
                .getPolicyFor(&app::models::Unmapped)
                .unwrap()
                .is_none()
        ),
    );
    observed.insert(
        "nonclass".into(),
        json!(default_registry.getPolicyFor(&42u32).unwrap().is_none()),
    );
    observed.insert(
        "empty_callback".into(),
        json!(
            registry
                .getPolicyFor(&app::models::Article)
                .unwrap()
                .is_none()
        ),
    );
    registry.guessPolicyNamesUsing(|_| {
        vec![
            "MissingPolicy".into(),
            type_name::<app::alternate::ArticlePolicy>().into(),
            type_name::<app::policies::ArticlePolicy>().into(),
        ]
    });
    observed.insert(
        "first_existing".into(),
        json!(
            registry
                .getPolicyFor(&app::models::Article)
                .unwrap()
                .unwrap()
                .name()
                == type_name::<app::alternate::ArticlePolicy>()
        ),
    );
    observed.insert(
        "callback_decision".into(),
        json!(
            !registry
                .forUser(None)
                .allows("view", &[&app::models::Article])
                .await
                .unwrap()
        ),
    );
    registry.policy::<app::models::Article>(app::policies::ArticlePolicy {
        sequence: 7,
        dependency: Arc::new(8),
    });
    observed.insert(
        "explicit".into(),
        json!(
            registry
                .getPolicyFor(&app::models::Article)
                .unwrap()
                .unwrap()
                .name()
                == type_name::<app::policies::ArticlePolicy>()
        ),
    );
    let mut snapshot = registry.policies();
    observed.insert(
        "mapping".into(),
        json!(
            snapshot.get(&TypeId::of::<app::models::Article>())
                == Some(&type_name::<app::policies::ArticlePolicy>())
        ),
    );
    snapshot.clear();
    observed.insert("snapshot".into(), json!(registry.policies().len() == 1));
    observed.insert(
        "other_app".into(),
        json!(default_registry.policies().is_empty()),
    );
    assert_eq!(
        Value::Object(observed),
        serde_json::from_str::<Value>(include_str!("reference/gate-policy-discovery.json"))
            .unwrap()
    );
}

#[tokio::test]
async fn discovery_propagates_factory_errors_and_does_not_resolve_before_interceptors() {
    let (app, _) = fixture().await;
    let registry = app.container().make::<GateRegistry>().unwrap();
    registry.discoverPolicy::<app::models::Article, BrokenPolicy>();
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = calls.clone();
    app.container()
        .bind::<BrokenPolicy, _>(move |_| {
            captured.fetch_add(1, Ordering::Relaxed);
            Err(openrust::contracts::container::BindingResolutionError::FacadeNotSet)
        })
        .unwrap();
    registry.guessPolicyNamesUsing(|_| {
        vec![
            type_name::<BrokenPolicy>().into(),
            type_name::<app::policies::ArticlePolicy>().into(),
        ]
    });
    assert!(registry.getPolicyFor(&app::models::Article).is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(
        registry
            .forUser(None)
            .inspect("view", &[&app::models::Article])
            .await
            .is_err()
    );
    let after = calls.load(Ordering::Relaxed);
    registry.before(|_, _, _| Box::pin(async { Ok(Some(Response::allow())) }));
    assert!(
        registry
            .forUser(None)
            .allows("view", &[&app::models::Article])
            .await
            .unwrap()
    );
    assert_eq!(calls.load(Ordering::Relaxed), after);
    registry.guessPolicyNamesUsing(|_| {
        vec![type_name::<app::models::policies::nested::EntryPolicy>().into()]
    });
    assert!(registry.getPolicyFor(&app::models::Article).is_err());
    assert!(
        GateRegistry::default()
            .resolvePolicy::<BrokenPolicy>()
            .is_err()
    );
    // 回调允许安全重入注册表，克隆forUser继续共享本应用定义。
    let weak = Arc::downgrade(&registry);
    registry.guessPolicyNamesUsing(move |_| {
        weak.upgrade()
            .unwrap()
            .defineGuest("from-callback", |_, _| {
                Box::pin(async { Ok(Response::allow()) })
            });
        Vec::new()
    });
    assert!(
        registry
            .getPolicyFor(&app::models::Article)
            .unwrap()
            .is_none()
    );
    assert!(registry.forUser(None).has("from-callback"));
}

#[tokio::test]
async fn discovered_policies_authorize_real_http_in_independent_applications() {
    use openrust::{
        foundation::configuration::Routing,
        http::{HttpError, HttpFuture, Request, Response as HttpResponse, StatusCode},
        support::facades::Route,
        testing::TestApp,
    };
    fn authorize(_: &mut Request) -> HttpFuture<'_, Result<HttpResponse, HttpError>> {
        Box::pin(async {
            assert!(Gate::getPolicyFor(&app::models::Article)?.is_some());
            Gate::forUser(None)?
                .authorize("view", &[&app::models::Article])
                .await?;
            HttpResponse::json(StatusCode::OK, json!({"allowed":true}))
        })
    }
    fn app(allowed: bool) -> TestApp {
        TestApp::create(move |builder| {
            builder
                .with_routing(
                    Routing::default().api(|| Route::get("/discovered-policy", authorize)),
                )
                .booted(move |app| {
                    app.container()
                        .bind::<app::policies::ArticlePolicy, _>(|_| {
                            Ok(Arc::new(app::policies::ArticlePolicy {
                                sequence: 0,
                                dependency: Arc::new(0),
                            }))
                        })?;
                    app.container()
                        .bind::<app::alternate::ArticlePolicy, _>(|_| {
                            Ok(Arc::new(app::alternate::ArticlePolicy))
                        })?;
                    Gate::discoverPolicy::<app::models::Article, app::policies::ArticlePolicy>()?;
                    Gate::discoverPolicy::<app::models::Article, app::alternate::ArticlePolicy>()?;
                    if !allowed {
                        Gate::guessPolicyNamesUsing(|_| {
                            vec![type_name::<app::alternate::ArticlePolicy>().into()]
                        })?;
                    }
                    Ok(())
                })
        })
        .unwrap()
    }
    let mut allowed = app(true);
    let mut denied = app(false);
    let (allowed, denied) = tokio::join!(
        allowed.get("/api/discovered-policy"),
        denied.get("/api/discovered-policy")
    );
    allowed
        .unwrap()
        .assertOk()
        .assertExactJson(&json!({"allowed":true}));
    denied.unwrap().assertForbidden();
}
