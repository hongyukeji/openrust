# Package Development

## Installation and service providers

Add the package as a Cargo dependency and list its provider in `bootstrap/providers.rs`. OpenRust compiles the provider and resource registrars into the application. The application explicitly selects providers when it assembles the framework.

In the provider's `register` hook, bind services and call `mergeConfigFrom(app, defaults, key)`. The application wins at the top level. Use `replaceConfigRecursivelyFrom(app, defaults, key)` to preserve nested defaults, with application arrays overriding by index. Pass a JSON object or array as the defaults. The key follows the application's dot notation.

## Resources

In the provider's `boot` hook, register resources with the application.

| Method | Resource |
| --- | --- |
| `loadRoutesFrom` | A compiled function that declares routes with `Route`. The application registers these routes after its own routes. |
| `loadViewsFrom` | A view directory and namespace. For `probe::welcome`, the app's `resources/views/vendor/probe/welcome.html` takes precedence over the package view when using the default view root. |
| `loadTranslationsFrom` | A translation directory and namespace. For `probe::messages.title`, `lang/vendor/probe/en/messages.json` can override the English title while other keys still fall back to the package. |
| `loadMigrationsFrom` | A static slice of compiled `MigrationEntry` values. Artisan applies and rolls back those entries using the application's migration ledger. |
| `commands` | A compiled function that registers commands with `Artisan` when the console kernel boots. |

Register package HTTP routes from an eager provider so they are available during route assembly. Ship the view and translation files with the application and pass their deployed directories to the provider. For a runnable host and provider, see the [independent crate fixture](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust/framework/tests/fixtures/package).

OpenRust uses explicit Cargo installation and provider registration. Automatic discovery and Artisan asset publishing are pending.
