//! 示例 API 路由；路由定义和处理器分开。

use super::RouteCapabilities;
use crate::modules::example::http::controllers;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
use controllers::auth::{
    auth_login, auth_logout, auth_refresh, auth_register, auth_request_email_verification,
    auth_request_password_reset, auth_reset_password, auth_verify_email, current_user,
};
use openrust::prelude::*;

pub(super) fn register(
    routes: Routes,
    capabilities: &RouteCapabilities,
    repository: openrust::config::repository::ConfigRepository,
) -> Routes {
    let routes = example_routes(routes);
    let routes = config_greeting_routes(routes, repository)
        .get("/api/example/route-url", controllers::example::route_url);
    let routes = route_context_routes(request_url_routes(routes))
        .get(
            "/api/example/representation",
            controllers::example::representation,
        )
        .post(
            "/api/example/request-body",
            controllers::example::request_body,
        );
    let routes = draft_redirect_routes(message_bag_routes(routes), capabilities)
        .post("/api/example/input", controllers::example::input)
        .name("example.input")
        .group(RouteGroup::new().middleware_alias("cookies"), |routes| {
            routes
                .get("/api/example/cookie", controllers::cookie::show)
                .post("/api/example/cookie", controllers::cookie::store)
        })
        .group(
            RouteGroup::new().middleware(openrust::http::InputNormalization::disabled()),
            |routes| {
                routes
                    .post("/api/example/input-raw", controllers::example::input_raw)
                    .name("example.input-raw")
            },
        )
        .group(
            RouteGroup::new()
                .domain("{account}.example.test")
                .prefix("/api/example/domain")
                .name_prefix("example.domain."),
            |routes| {
                routes
                    .get("/orders/{id}", controllers::example::domain_route)
                    .name("orders.show")
            },
        )
        .group(
            RouteGroup::new().middleware_group("example.response"),
            |routes| routes.get("/api/example/middleware", controllers::example::middleware),
        );
    #[cfg(feature = "session")]
    let routes = draft_session_routes(routes, capabilities.session);
    let routes = if capabilities.i18n {
        routes
            .get(
                "/api/example/heartbeat-label",
                controllers::example::heartbeat_label,
            )
            .get(
                "/api/example/reading-summary",
                controllers::localization::show,
            )
    } else {
        routes
    };
    #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
    let routes = if capabilities.database {
        article_routes(
            tenant_routes(account_routes(auth_routes(routes), capabilities)),
            capabilities.summary_origin.is_some(),
        )
    } else {
        routes
    };
    #[cfg(all(
        feature = "auth-oidc",
        any(feature = "db-postgres", feature = "db-mysql")
    ))]
    let routes = if capabilities.database && capabilities.oidc {
        oidc_routes(routes)
    } else {
        routes
    };
    #[cfg(feature = "queue")]
    let routes = if capabilities.database {
        routes.post(
            "/api/example/jobs/heartbeat",
            controllers::jobs::enqueue_example_heartbeat,
        )
    } else {
        routes
    };
    #[cfg(not(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql"))))]
    let _ = &capabilities.summary_origin;
    #[cfg(not(any(
        feature = "queue",
        all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql"))
    )))]
    let _ = capabilities.database;
    routes
}

fn example_routes(routes: Routes) -> Routes {
    routes
        .get("/api/example", controllers::example::show)
        .name("example.index")
        .get(
            "/api/example/module-state",
            crate::modules::example::providers::module_state::show,
        )
        .get(
            "/api/example/exception",
            controllers::example::exception_example,
        )
}

fn config_greeting_routes(
    routes: Routes,
    repository: openrust::config::repository::ConfigRepository,
) -> Routes {
    routes
        .get("/api/example/config-feature", {
            let repository = repository.clone();
            move || {
                let repository = repository.clone();
                async move { controllers::config_reading::feature(&repository) }
            }
        })
        .get("/api/example/config-greeting", {
            let repository = repository.clone();
            move || {
                let repository = repository.clone();
                async move { controllers::config_reading::show(&repository) }
            }
        })
        .post("/api/example/config-greeting/review", move || {
            let repository = repository.clone();
            async move { controllers::config_reading::review(&repository) }
        })
}

fn message_bag_routes(routes: Routes) -> Routes {
    routes.post(
        "/api/example/message-bag",
        controllers::example::message_bag,
    )
}

fn draft_redirect_routes(routes: Routes, capabilities: &RouteCapabilities) -> Routes {
    routes
        .get(
            "/api/example/draft/signed-links",
            controllers::draft_redirect::signed_issue,
        )
        .group(RouteGroup::new(), |signed| {
            signed
                .get(
                    "/api/example/draft/signed/{id}",
                    controllers::draft_redirect::signed_show,
                )
                .name("example.draft.signed")
                .middleware_alias("signed")
        })
        .group(RouteGroup::new(), |signed| {
            signed
                .get(
                    "/api/example/draft/signed-relative/{id}",
                    controllers::draft_redirect::signed_relative_show,
                )
                .name("example.draft.signed.relative")
                .middleware_alias("signed:relative")
        })
        .group(
            RouteGroup::new()
                .prefix("/api/example/draft/actions")
                .namespace("Example\\Draft"),
            |drafts| {
                drafts
                    // 覆盖默认概览；静态路径先于参数路径，按 Laravel 声明顺序匹配。
                    .get("/summary", controllers::draft_redirect::summary_default)
                    .get_action(
                        "/summary",
                        "DraftSummaryController",
                        controllers::draft_redirect::summary,
                    )
                    .group(draft_session_group(capabilities), |drafts| {
                        drafts
                            .get_action(
                                "/{id}",
                                "DraftController@show",
                                controllers::draft_redirect::action_show,
                            )
                            .get_action(
                                "/{id}/preview",
                                "PreviewDraftController",
                                controllers::draft_redirect::action_show,
                            )
                    })
            },
        )
        .get(
            "/api/example/draft/action",
            controllers::draft_redirect::action,
        )
        .get(
            "/api/example/draft/action-preview",
            controllers::draft_redirect::action_preview,
        )
        .group(draft_session_group(capabilities), |routes| {
            routes.post(
                "/api/example/draft/action-submit",
                controllers::draft_redirect::action_submit,
            )
        })
        .get(
            "/api/example/draft/plain",
            controllers::draft_redirect::plain,
        )
        .get("/api/example/draft/back", controllers::draft_redirect::back)
        .get(
            "/api/example/draft/guide",
            controllers::draft_redirect::guide,
        )
        .group(draft_session_group(capabilities), |routes| {
            routes
                .get(
                    "/api/example/draft/refresh",
                    controllers::draft_redirect::refreshed,
                )
                .post(
                    "/api/example/draft/refresh",
                    controllers::draft_redirect::refresh,
                )
        })
        .get(
            "/api/example/draft/secure",
            controllers::draft_redirect::secure,
        )
        .get(
            "/api/example/draft/named",
            controllers::draft_redirect::named,
        )
}

// API 草稿仅在应用提供 Session 时显式启用状态，不加入 Web CSRF 组。
fn draft_session_group(capabilities: &RouteCapabilities) -> RouteGroup {
    let group = RouteGroup::new();
    #[cfg(feature = "session")]
    if capabilities.session {
        return group
            .middleware_alias("cookies")
            .middleware_alias("session");
    }
    #[cfg(not(feature = "session"))]
    let _ = capabilities;
    group
}

#[cfg(feature = "session")]
fn draft_session_routes(routes: Routes, session_enabled: bool) -> Routes {
    let routes = routes.group(
        if session_enabled {
            RouteGroup::new()
                .middleware_alias("cookies")
                .middleware_alias("session")
        } else {
            RouteGroup::new()
        },
        |routes| {
            routes.post(
                "/api/example/draft/submit",
                controllers::draft_redirect::submit,
            )
        },
    );
    if session_enabled {
        session_routes(routes)
    } else {
        routes
    }
}

#[cfg(feature = "session")]
fn session_routes(routes: Routes) -> Routes {
    routes.group(
        RouteGroup::new()
            .middleware_alias("cookies")
            .middleware_alias("session"),
        |routes| {
            routes
                .get(
                    "/api/example/draft/review",
                    controllers::draft_redirect::review,
                )
                .name("example.draft.review")
                .get(
                    "/api/example/draft/preview",
                    controllers::draft_redirect::preview,
                )
                .get(
                    "/api/example/draft/confirm",
                    controllers::draft_redirect::confirmation,
                )
                .post(
                    "/api/example/draft/confirm",
                    controllers::draft_redirect::confirm,
                )
                .get("/api/example/session", controllers::session::show)
                .post("/api/example/session", controllers::session::store)
                .get(
                    "/api/example/session/old-input",
                    controllers::session::old_input,
                )
                .post(
                    "/api/example/session/old-input",
                    controllers::session::old_input_store,
                )
                .post(
                    "/api/example/session/previous",
                    controllers::session::previous,
                )
                .get(
                    "/api/example/session/previous/named",
                    controllers::session::previous_visit,
                )
                .name("example.session.previous.named")
                .get(
                    "/api/example/session/previous/anonymous",
                    controllers::session::previous_visit,
                )
                .group(
                    RouteGroup::new()
                        .prefix("/api/example/session/previous")
                        .middleware_alias("precognitive"),
                    |routes| {
                        routes
                            .get("/preview", controllers::session::previous_visit)
                            .name("example.session.previous.preview")
                    },
                )
                .group(
                    RouteGroup::new()
                        .prefix("/api/example/session/old-input")
                        .middleware_alias("precognitive"),
                    |routes| routes.post("/preview", controllers::session::old_input_store),
                )
        },
    )
}

fn route_context_routes(routes: Routes) -> Routes {
    routes.group(
        RouteGroup::new()
            .prefix("/api/example/route-context")
            .middleware(openrust::http::axum::middleware::from_fn(
                controllers::example::route_context_middleware,
            )),
        |routes| {
            routes
                .route(
                    "/named/{id}",
                    openrust::http::routing::get(controllers::example::route_context)
                        .post(controllers::example::route_context),
                )
                .name("example.route-context.show")
                .get("/anonymous/{id}", controllers::example::route_context)
        },
    )
}

fn request_url_routes(routes: Routes) -> Routes {
    routes
        .get(
            "/api/example/request-url",
            controllers::example::request_url,
        )
        .post(
            "/api/example/request-url",
            controllers::example::request_url,
        )
}

pub(crate) async fn response_marker_middleware(
    request: openrust::http::axum::extract::Request,
    next: openrust::http::axum::middleware::Next,
) -> openrust::http::axum::response::Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-example-middleware",
        openrust::http::axum::http::HeaderValue::from_static("named-group"),
    );
    response
}

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
fn tenant_routes(routes: Routes) -> Routes {
    use controllers::tenant;
    use openrust::auth::{AuthLayer, AuthMode};

    routes.group(
        RouteGroup::new()
            .prefix("/api/example/tenant")
            .middleware(AuthLayer::required(AuthMode::Jwt))
            .middleware(openrust::http::axum::middleware::from_fn(
                tenant::verify_membership,
            ))
            .middleware(tenant::trusted_layer().layer()),
        |api| {
            let api = api
                .get("/record", tenant::show)
                .put("/record", tenant::update);
            #[cfg(feature = "queue")]
            let api = api.post("/record/rebuild", tenant::dispatch);
            #[cfg(any(feature = "storage-local", feature = "storage-s3"))]
            let api = api
                .get("/record/file", tenant::read_file)
                .put("/record/file", tenant::write_file);
            api
        },
    )
}

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
async fn private_account_response(
    request: openrust::http::axum::extract::Request,
    next: openrust::http::axum::middleware::Next,
) -> openrust::http::axum::response::Response {
    use openrust::http::axum::http::{HeaderValue, header};
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
fn account_routes(routes: Routes, capabilities: &RouteCapabilities) -> Routes {
    use controllers::account_security as account;
    use openrust::auth::{AuthLayer, AuthMode};
    #[cfg(feature = "session")]
    let session_enabled = capabilities.session;
    #[cfg(not(feature = "session"))]
    let session_enabled = {
        let _ = capabilities;
        false
    };
    routes.nest("/api/example/account", |api| {
        let api = api.post("/login", account::login);
        let api = if session_enabled {
            api.group(
                RouteGroup::new()
                    .middleware_alias("cookies")
                    .middleware_alias("session"),
                |routes| routes.post("/session/login", account::session_login),
            )
        } else {
            api
        };
        #[cfg(feature = "auth-mfa")]
        let api = api.post("/login/mfa", account::complete);
        let api = if session_enabled {
            api.group(
                RouteGroup::new()
                    .middleware_alias("cookies")
                    .middleware_alias("session"),
                |routes| {
                    routes
                        .get("/session/preference", account::preference)
                        .post("/session/preference", account::set_preference)
                        .post("/session/logout", account::session_logout)
                        .layer(AuthLayer::required(AuthMode::Session))
                },
            )
        } else {
            api
        };
        #[cfg(feature = "auth-mfa")]
        let api = api.merge(
            Routes::new()
                .post("/mfa/enroll", account::enroll)
                .post("/mfa/confirm", account::confirm)
                .post("/mfa/recovery", account::regenerate)
                .post("/mfa/disable", account::disable)
                .layer(AuthLayer::required(AuthMode::Jwt)),
        );
        api.layer(openrust::http::axum::middleware::from_fn(
            private_account_response,
        ))
    })
}

#[cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
fn oidc_routes(routes: Routes) -> Routes {
    use controllers::oidc;
    use openrust::auth::{AuthLayer, AuthMode};

    routes.nest("/api/example/oidc", |api| {
        let api = api.group(
            RouteGroup::new()
                .middleware_alias("cookies")
                .middleware_alias("session"),
            |routes| {
                routes
                    .post("/bind/authorize", oidc::browser_bind_authorize)
                    .post("/bind/confirm", oidc::browser_bind_confirm)
                    .post("/session/attach", oidc::attach_session)
                    .layer(AuthLayer::required(AuthMode::Session))
            },
        );
        let protected = Routes::new()
            .get("/links", oidc::linked)
            .post("/bind", oidc::bind)
            .post("/unbind", oidc::unbind)
            .post("/logout", oidc::logout)
            .layer(AuthLayer::required(AuthMode::Jwt));
        api.get("/authorize", oidc::authorize)
            .get("/callback", oidc::callback)
            .post("/backchannel", oidc::backchannel)
            .merge(protected)
            .layer(openrust::http::axum::middleware::from_fn(
                private_oidc_response,
            ))
    })
}

#[cfg(all(
    feature = "auth-oidc",
    any(feature = "db-postgres", feature = "db-mysql")
))]
async fn private_oidc_response(
    request: openrust::http::axum::extract::Request,
    next: openrust::http::axum::middleware::Next,
) -> openrust::http::axum::response::Response {
    use openrust::http::axum::http::{HeaderValue, header};
    let mut response = private_account_response(request, next).await;
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
#[allow(clippy::too_many_lines)] // 文章同一认证组的路由装配保留在一个可审查入口。
fn article_routes(routes: Routes, summary_enabled: bool) -> Routes {
    use controllers::article;

    let routes = routes.group(
        RouteGroup::new()
            .prefix("/api/example")
            .middleware_alias("precognitive")
            .middleware_alias("example.article-auth"),
        |routes| {
            routes.resource_with::<article::Articles>("/articles", |actions| {
                actions.only(&[openrust::http::Action::Create])
            })
        },
    );
    let routes = routes.nest("/api/example", |api| {
        #[cfg(feature = "queue")]
        let api = api
            .get(
                "/articles/{id}/cached-title",
                controllers::article_cache::show,
            )
            .get(
                "/articles/{id}/cached-title/swr",
                controllers::article_cache::show_swr,
            );
        #[cfg(all(
            feature = "realtime",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        let api = api
            .get("/articles/{id}/events", controllers::realtime::events)
            .get(
                "/articles/{id}/socket",
                controllers::article_socket::connect,
            )
            .merge(
                Routes::new()
                    .post("/articles/{id}/announce", controllers::realtime::announce)
                    .layer(
                        openrust::http::RateLimitLayer::by_user(
                            "example:article:announce",
                            30,
                            std::time::Duration::from_secs(60),
                        )
                        .layer(),
                    ),
            );
        #[cfg(feature = "data-exchange")]
        let api = api
            .post("/articles/import", controllers::article_transfer::import)
            .get("/articles/export", controllers::article_transfer::export);
        #[cfg(all(feature = "data-exchange-tasks", feature = "storage-local"))]
        let api = api
            .post(
                "/articles/export-jobs",
                controllers::article_transfer::dispatch_export,
            )
            .get(
                "/articles/export-jobs/{job_id}/{attempt}",
                controllers::article_transfer::download_export,
            )
            .post(
                "/articles/import-jobs",
                controllers::article_transfer::dispatch_import,
            )
            .get(
                "/articles/import-jobs/{import_id}/{job_id}/{attempt}",
                controllers::article_transfer::import_report,
            )
            .get(
                "/articles/import-jobs/{import_id}/status",
                controllers::article_transfer::import_status,
            );
        #[cfg(feature = "http-client")]
        let api = if summary_enabled {
            api.get("/articles/{id}/summary", article::external_summary)
        } else {
            api
        };
        #[cfg(not(feature = "http-client"))]
        let _ = summary_enabled;
        #[cfg(all(
            feature = "storage-local",
            any(feature = "db-postgres", feature = "db-mysql")
        ))]
        let api = api
            .post(
                "/articles/{id}/attachment/file",
                controllers::attachments::upload_multipart,
            )
            .put(
                "/articles/{id}/attachment",
                controllers::attachments::upload,
            )
            .get(
                "/articles/{id}/attachment",
                controllers::attachments::download,
            )
            .delete(
                "/articles/{id}/attachment",
                controllers::attachments::delete,
            )
            .put(
                "/articles/{id}/attachment/visibility",
                controllers::attachments::set_visibility,
            );
        #[cfg(feature = "encryption")]
        let api = api
            .put("/articles/{id}/private-note", article::put_private_note)
            .get("/articles/{id}/private-note", article::get_private_note);
        #[cfg(feature = "signing")]
        let api = api
            .post("/articles/{id}/share", article::issue_share)
            .post("/articles/{id}/share/revoke", article::revoke_share);
        api.merge(
            Routes::new()
                .post("/welcome", controllers::welcome::send)
                .layer(
                    openrust::http::RateLimitLayer::by_user(
                        "example:welcome",
                        3,
                        std::time::Duration::from_secs(3600),
                    )
                    .layer(),
                ),
        )
        // export 等静态业务入口先登记，再挂载包含 /articles/{id} 的资源路由。
        .resource_with::<article::Articles>("/articles", |actions| {
            actions.except(&[openrust::http::Action::Create])
        })
        .resource::<controllers::example_record::ExampleRecords>("/records")
        .layer(openrust::auth::AuthLayer::required(
            openrust::auth::AuthMode::Jwt,
        ))
    });
    #[cfg(feature = "realtime")]
    let routes = routes.nest("/api/example", |api| {
        api.get(
            "/articles/{id}/browser-socket",
            controllers::article_socket::browser_connect,
        )
        .layer(openrust::auth::AuthLayer::required(
            openrust::auth::AuthMode::Session,
        ))
    });
    #[cfg(feature = "signing")]
    let routes = routes.get("/api/example/shared-articles/{token}", article::shared_show);
    routes
}

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
fn auth_routes(routes: Routes) -> Routes {
    routes.nest("/api/example", |api| {
        api.get("/me", current_user)
            .layer(openrust::auth::AuthLayer::required(
                openrust::auth::AuthMode::Jwt,
            ))
            .post("/auth/register", auth_register)
            .post(
                "/auth/request-email-verification",
                auth_request_email_verification,
            )
            .post("/auth/verify-email", auth_verify_email)
            .post("/auth/login", auth_login)
            .post("/auth/refresh", auth_refresh)
            .post("/auth/logout", auth_logout)
            .post("/auth/password-reset", auth_request_password_reset)
            .post("/auth/password-reset/confirm", auth_reset_password)
            .merge(Routes::new().get("/me/pat", current_user).layer(
                openrust::auth::AuthLayer::required(openrust::auth::AuthMode::PersonalAccessToken),
            ))
    })
}
