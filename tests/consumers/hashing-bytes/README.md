# Hashing 字节输入外包

只依赖公开 `Hasher` / `HashManager` / `extend`，自定义测试驱动观察 make、check、能力声明的原始字节及 trait-object 可用性。

`driver_for_operation()` 回归通过两层管理器取得原扩展驱动，普通扩展无需实现 `as_hash_manager`；能力检查、生成与校验复用同一最终驱动，并检查借用 future 的 `Send` 约束。测试摘要是公开合成占位符，不实现密码算法，也不用于认证。

在项目独立验收目录执行 `cargo test --manifest-path tests/consumers/hashing-bytes/Cargo.toml`；内置驱动与 PHP 互通另由框架实际用例验收。
