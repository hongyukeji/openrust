# Package Development

## Installation and service providers

Add the package as a Cargo dependency. OpenRust discovers packages declared for the active target and features. For a manual registration, add the provider in `bootstrap/providers.rs`.

In the provider's `register` hook, bind services and call `merge_config_from(app, defaults, key)`. The application wins at the top level. Use `replace_config_recursively_from(app, defaults, key)` to preserve nested defaults, with application arrays overriding by index. Pass a JSON object or array as the defaults. The key follows the application's dot notation.

## Package discovery {#package-discovery}

A package opts in through its Cargo manifest and exports `openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>>`. This example also exports a typed Rust alias.

```toml
[package.metadata.openrust.discovery]
version = 1
providers = true
aliases = { ExampleFacade = "facades::Example" }
```

OpenRust applications run `openrust_package_discovery::generate()` from `build.rs` and install the generated catalog through `with_packages`. The application compiles the factories and aliases. An executable can start from the linked catalog in a deployment without Cargo.

`bootstrap/packages.rs` exposes the compiled catalog through `manifest()`. The application entry point calls `.with_packages(super::packages::manifest())`; register application providers separately in `bootstrap/providers.rs`. Do not edit generated files or manually list discovered packages. Compiled Rust aliases are available through `crate::bootstrap::packages::package_aliases`; their availability follows the selected features and discovery exclusions.

Add `dont-discover = ["example-package"]` under the application’s `[package.metadata.openrust.discovery]` with `version = 1`. Use `["*"]` to opt out of all discovery. A package can also declare package names to exclude. A manually registered provider still runs when its package is excluded.

HTTP and Artisan startup use the linked catalog directly. To inspect the packages linked into a build, run `./artisan package:discover` to list their names and export the catalog to `bootstrap/cache/packages.json`. Set `APP_PACKAGES_CACHE` to choose a different export path. Run `./artisan clear-compiled` to remove the exported file. Rebuild the executable after changing dependencies, features, or exclusions.

After the application resolves its environment, `get_cached_packages_path()` returns the export path. The directory of deferred service providers is linked into the executable.

## Resources

In the provider's `boot` hook, register resources with the application.

| Method | Resource |
| --- | --- |
| `load_routes_from` | A compiled function that declares routes with `Route`. The application registers these routes after its own routes. |
| `load_views_from` | A view directory and namespace. For `probe::welcome`, the app's `resources/views/vendor/probe/welcome.html` takes precedence over the package view when using the default view root. |
| `load_translations_from` | A translation directory and namespace. For `probe::messages.title`, `lang/vendor/probe/en/messages.json` can override the English title while other keys still fall back to the package. |
| `load_json_translations_from` | An anonymous JSON translation root. The application’s locale JSON overrides matching package keys. |
| `load_view_components_as` | A compiled component registered by prefix and alias. For example, prefix `probe` and alias `badge` render as `<x-probe-badge />`. Anonymous components under a package’s `views/components` are available as `<x-probe::badge />`. |
| `load_migrations_from` | A static slice of compiled `MigrationEntry` values. Artisan applies and rolls back those entries using the application's migration ledger. |
| `commands` | A compiled function that registers commands with `Artisan` when the console kernel boots. |

Register package HTTP routes from an eager provider so they are available during route assembly. Ship the view and translation files with the application and pass their deployed directories to the provider. For a runnable host and provider, see the [independent crate fixture](https://github.com/hongyukeji/openrust/tree/master/vendor/openrust/framework/tests/fixtures/package).

## Publishing assets

Use `publishes` for embedded files with application-relative destinations. Use `publishes_paths` for file or directory mappings with application-relative or explicitly configured absolute destinations, such as `app.lang_path("vendor/package")`. Run `./artisan vendor:publish` and start typing to filter providers, tags, or all assets. Move with ↑ and ↓, confirm with Enter, or cancel with Esc. Choose directly with `--provider`, `--tag`, or `--all`. Directory publishing preserves relative file paths. Existing files are preserved by default. Use `--force` to overwrite them or `--existing` to update only files already present.

Combine `--provider` with one or more `--tag` options to select assets by source path. `--all` publishes across providers and tags. `--existing` limits the selection to files already present even when combined with `--force`. Relative source paths resolve within the application. A provider can supply absolute paths for resources installed elsewhere. Within the application, publishing refuses symbolic links in source and destination paths, including during migration name lookup.
