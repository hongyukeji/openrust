# OpenRust dependency patch

Source: published crypto-glue 0.2.2 (MPL-2.0), upstream commit e40f0c6ecd5557550fec8ebbd2861521d92ee1a5. The only Rust source adaptation replaces the deprecated RSA type imports with equivalent stable type names. Cargo.toml changes RSA's version to =0.10.0 and its feature from sha2 to pkcs1v15. RSA 0.10.0 always includes SHA-2; its PKCS#1 v1.5 module now requires the pkcs1v15 feature. This resolves the upstream manifest conflict while consuming the stable Marvin fix. Remove this patch when an upstream crypto-glue release supports stable RSA 0.10.0 directly.

The complete MPL-2.0 text is included in LICENSE-MPL-2.0; upstream licensing and source headers remain intact. Cargo.toml.orig and the original lockfile are upstream references, not the application's dependency resolution. The root Cargo.lock defines the accepted build.
