# Asset Bundling / Vite

OpenRust resolves JavaScript/CSS entry manifests and generates development or production resource tags through `openrust::support::facades::Vite`. The underlying service is `openrust::foundation::Vite`; no extra installation command is needed. A frontend project still installs and runs Vite itself. Font configuration through `@fonts` remains available in [Blade Templates](blade.md).

## Blade entry points {#available-entry-points}

```blade
@vite(['resources/css/app.css', 'resources/js/app.js'])
@viteReactRefresh

<img src="{{ Vite::asset('resources/images/logo.png') }}">
<meta name="csp-nonce" content="{{ Vite::cspNonce() }}">
```

## Asset setup {#configuration}

Configure application defaults in a service provider’s `boot`. The default build directory is `public/build`, the manifest filename is `manifest.json`, and the development marker is `public/hot`. `use_hot_file(path)` selects a different marker. `@vite` accepts one entry string or a list, and an optional build directory as its second argument. `with_entry_points` and `merge_entry_points` configure `to_html()`; normal `@vite` calls use their own explicit entries.

```rust
use openrust::support::facades::Vite;

Vite::use_build_directory("build")?;
Vite::use_manifest_filename("manifest.json")?;
Vite::with_entry_points(["resources/js/app.js"])?;
Vite::use_script_tag_attributes([
    ("defer".into(), true.into()),
    ("data-app".into(), "OpenRust".into()),
].into())?;
```

## Production and development {#asset-modes}

Build your frontend with a manifest and place its output under the configured public build directory. `Vite::render(entries, None)` returns an `HtmlString`; `to_html` returns a string. Static imports and their CSS are resolved recursively, shared tags are deduplicated, CSS tags precede entry scripts, and preload tags and integrity hashes are generated from the actual manifest. `preloaded_assets()` returns the generated preload attributes. `flush()` clears this execution state, retaining configuration and the application’s manifest cache.

When the hot marker exists, its content supplies the dev-server URL: tags include `@vite/client` and the requested entries. `react_refresh()` / `@viteReactRefresh` add the React preamble only in hot mode. `asset(path, build)` returns a URL; `content(path, build)` returns original file bytes and continues to use the production manifest even in hot mode. Both build arguments are optional via `None`. `manifest_hash(None)` returns the current file MD5 or `None` when hot or missing; it reads the current file independently of the parsed manifest cache.

## Callbacks and isolation {#callbacks-and-lifecycle}

Use `create_asset_paths_using` for custom asset URLs, and `clear_asset_path_resolver` to restore the URL generator. Static tag attribute maps or `use_script_tag_attributes_using` / `use_style_tag_attributes_using` callbacks receive `TagContext` with the actual entry, URL, chunk and manifest. Hot mode has no chunk or manifest. `use_integrity_key(Some("integrity"))` selects the field; `None` disables integrity. `use_preload_tag_attributes(None)` disables preloads. The preload callback form returns `Option<Attributes>`: `None` stops that tag.

Callbacks can reenter configuration without holding its lock. Use the `*_using_fallible` variants for attribute callbacks returning `ViteResult`, preserving an actual error with `ViteError::callback(error)`. Attribute values support scalar text/numbers, true for a standalone attribute, and false/null for omission. Values are trusted application configuration; nested attributes return `InvalidAttribute`. The tag is generated after its attribute callback, so callback changes to the nonce apply at the same point as the pinned contract.

Provider defaults persist. HTTP, Artisan, and each Worker job receive their own execution configuration, nonce and preload state; overrides in one lifecycle do not become defaults for the next. In HTTP middleware call `Vite::use_csp_nonce(None)?` to generate a 40-character nonce, then set your response CSP header from the returned value. When a route lends its mutable request to view or response facades, use the existing `request.with_container(async { ... }).await` pattern.

## Fonts {#fonts}

`Vite::fonts(None)` returns an `HtmlString` for all families. Select ordered aliases with `Vite::fonts(Some(&["sans", "mono"]))?`; use `Some(&[])` for none. `@fonts` and `Vite::fonts` delegate to this same service and share preload deduplication with ordinary Vite assets.

The production font manifest defaults to `public/build/fonts-manifest.json`; set `use_build_directory` and `use_fonts_manifest_filename` in your provider to change it. In hot mode, `fonts-manifest.dev.json` is read next to the configured hot marker, including a custom marker outside the public root. A missing manifest emits no tags.

The version 1 manifest contains `families`, `preloads` and optional `style`. Filtered calls emit `familyStyles` and `variables` in alias order; unfiltered calls use `style.inline`, then `style.file`. Unknown aliases and invalid selected preloads return errors. Preload callbacks receive `source = "fonts"`, an empty chunk and an empty manifest, and may modify or disable font links. CSP nonces apply to both links and inline styles.

`flush()` also clears the application’s parsed font manifests and preload state; it retains settings and the JavaScript/CSS manifest cache. Missing files and JSON parse failures are not cached. Font CSS file reads retain the public-directory capability boundary, and CSS containing a style-closing tag is rejected. Manifest attributes and inline CSS are trusted build output.

```rust
Vite::use_fonts_manifest_filename("fonts-manifest.json")?;
let fonts = Vite::fonts(Some(&["sans", "mono"]))?;
```

## Prefetching {#prefetching}

`prefetch(None, None)` enables aggressive prefetching on the browser `load` event. Pass `Some(3)` to use waterfall loading with three concurrent links, or `Some("vite:ready")` as the second argument for a custom window event.

`use_waterfall_prefetching(None)` reuses the previous concurrency, initially 3; `use_aggressive_prefetching()` switches to all-at-once loading. `use_prefetch_strategy(None, None)` disables prefetching; `Some("waterfall")` and `Some("aggressive")` are the supported strategy names. An unknown strategy fails at render time. Hot mode emits no prefetch script.

The production manifest’s dynamic imports, their imports and CSS are discovered in order, deduplicated by URL and filtered against already-preloaded assets. The script runs only after its event. Waterfall releases one slot after either a load or error; aggressive loading inserts all links together. Existing preload attribute resolvers also apply, and the generated script uses the current CSP nonce.

When prefetching is enabled, returning `None` from a preload attribute resolver produces `DisabledPrefetchAttributes`, matching the pinned source’s unpacking failure. Use supported attribute maps or disable the prefetch strategy explicitly; a failed render is not silently treated as success. Event names and tag attributes are trusted application configuration.

```rust
Vite::prefetch(Some(3), Some("vite:ready"))?;
Vite::use_prefetch_strategy(None, None)?;
```

## Boundaries {#support-status}

Typed Vite macros and inherited Facade test helpers remain under review.

## Scope {#scope}

These rendering calls do not launch or supervise a Node development server.
