# SQLx MySQL driver 0.9.0

This is the MySQL driver from the published [sqlx-mysql 0.9.0 crate](https://crates.io/crates/sqlx-mysql/0.9.0), including its original MIT and Apache-2.0 licenses. The upstream archive SHA-256 is `90b8020fe17c5f2c245bfa2505d7ef59c5604839527c740266ad2214acebea27`.

The driver contains the source portion of [SQLx #4334](https://github.com/transact-rs/sqlx/commit/2cd6369be3ca1f69cd68ac71ff466486d8e3040a). It exposes `MySqlConnectOptions::found_rows` with the upstream default. OpenRust opts out for its own MySQL and MariaDB connections to use Laravel's affected-row semantics. The remaining SQLx crates still resolve from the locked registry versions.

When a supported SQLx release includes this option, remove the workspace patch and use that release.
