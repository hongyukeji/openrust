# OpenRust Pennant

Runtime feature flags following the framework Pennant's driver, scoped interaction and event contracts. Uses the native OpenRust container, database, authentication, Artisan and middleware services.

Enable the application's `pennant` Cargo feature. Package discovery registers `PennantServiceProvider`; definitions belong in application Providers. The default database store uses the `features` migration. The array store lives within one HTTP request, Artisan call or queue job.

See the [English guide](../../docs/en/packages/pennant.md), [简体中文](../../docs/zh-Hans/packages/pennant.md) or [繁體中文](../../docs/zh-Hant/packages/pennant.md) for configuration, feature classes, explicit scopes, authentication and commands.

PHP dynamic loading is expressed through compiled Rust modules and asynchronous factories. Lazy authentication requires a native Request via `Feature::for_request`; the middleware resolves it automatically. Only JSON `false` is inactive. Arbitrary PHP objects must be represented by serializable JSON feature values.
