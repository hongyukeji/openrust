# SeaORM 有符号执行结果适配

固定 crates.io `sea-orm` 2.0.4；原始crate SHA-256 `40bc08e0aa1963295a30473f98d9b4c9c2b0b3a028381e0143cb2647d0a59a32`，原文件散列见 `upstream-sha256.json`，MIT/Apache-2.0许可证原样保留。

唯一生产改动位于 `src/executor/execute.rs`：增加 `ExecResult::try_last_insert_id`，SQLx/rusqlite 的 SQLite rowid 保留 i64，MySQL保留u64，PG等不支持结果返回DbErr而非panic。复用本次执行返回的驱动结果，不发SQL、不换连接、不复制执行器。原 `last_insert_id` API 保留；OpenRust新Query入口只用安全访问器。

原因：上游旧访问器会对合法负SQLite rowid触发unreachable。不能为映射Laravel insertGetId而限定非负、catch panic或通过其他连接SELECT推断。正式上游修复/版本升级后须核替换。根及独立应用必须解析同一源码patch，Cargo不会传播依赖包的patch；当前适配未证明crates.io发布可用。
