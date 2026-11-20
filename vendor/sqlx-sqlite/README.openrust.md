# OpenRust dependency patch

Source: published sqlx-sqlite 0.9.0, under MIT OR Apache-2.0. Source files are unmodified. Cargo.toml changes only libsqlite3-sys from >=0.30.1,<0.38.0 to =0.39.1 (SQLite 3.54.1), addressing RUSTSEC-2026-0459. SQLx 0.9.1 also widens this bound to include the fixed version; this minimal backport preserves the currently accepted SQLx core, PostgreSQL and patched MySQL driver behavior. Remove this patch when the complete SQLx 0.9.1 driver update is accepted. Both upstream licenses are retained.

Cargo.toml.orig and the original lockfile are upstream references; root Cargo.lock defines the application build.
