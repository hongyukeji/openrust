# API reference

The guides explain application behavior. Rustdoc lists the public Rust modules, types, methods, feature gates, and source links for the exact workspace snapshot used to build it.

`openrust-translation-database` provides `DatabaseLoader` for explicitly loading database translations into the existing translator. See [database translations](../digging-deeper/localization.md#database-translation-loader) for setup, file fallback, and supported database targets.

`openrust-settings` provides `Settings<T>`, `SettingsGroup`, and `SettingsServiceProvider` for typed database settings. See [database settings](../packages/settings.md) for registration, migrations, encryption, and transaction and scope behavior.

## Build a source snapshot

Use Python 3.11 or newer, Rust 1.94.0, and a clean committed source snapshot. From the repository root, choose a new output directory and a dedicated Cargo artifact directory:

```sh
python3 -B docs/scripts/build_api.py --output /tmp/openrust-api --target-dir /tmp/openrust-api-target
```

For an exported source archive without Git metadata, add `--revision` with the full 40-character commit SHA of that archive. The command uses the lockfile, documents every workspace library with all features, and rejects broken or private intra-doc links. It clears the dedicated target’s Rustdoc pages before rebuilding so removed APIs cannot remain in the export, while retaining Cargo dependency artifacts. It does not overwrite an existing export. The root binary is not a library API.

The export targets the build host; `build-info.json` records its target triple. Platform-specific APIs follow that target’s conditional compilation. The script uses the host even if `CARGO_BUILD_TARGET` is set. All features expose optional APIs for reference; applications still enable the features they need in Cargo.toml. Dependency APIs link to external Rustdoc and are not bundled. The export contains local search, source pages, and `build-info.json` with the revision, Rust version, packages, lockfile hash, and source fingerprints.

## Include API pages in the site

```sh
cd docs
DOCS_API_DIR=/tmp/openrust-api npm run build
```

The build checks the export against the current lockfile and source fingerprints, then copies it to `api/`. The sidebar shows a Rustdoc entry only when an export is supplied. It works at both the site root and a deployment subdirectory such as `/docs/`. A normal build keeps this guide available without an API export. To preview the static export alone, serve its directory using a local HTTP server and open `index.html`.

API generation and site generation do not publish a release. Rebuild the reference when library sources, workspace manifests, or the lockfile change. Keep release artifacts tied to their recorded source revision; see [releases](../prologue/releases.md) and [deployment](../getting-started/deployment.md).
