# 缓存

当前实现位于根 `openrust` package 的 `openrust::cache`。

- `CachePlugin` 在 `build` 阶段安装服务和 built-in driver，在 `finish` 阶段读取 `[cache]`、解析所有 store 并初始化 backend；默认 store 缺失、未知 driver、未知 driver option 或连接失败均启动失败。
- `Cache` 提供 default 与按名选择的 `CacheStore`；值通过 JSON 编解码，miss 为 `None`，序列化和 I/O 错误保留为错误。key 限 1–1024 UTF-8 bytes，零 TTL 拒绝。
- `memory` 作用域为进程；`file` 使用 SHA-256 文件名、毫秒 TTL envelope 与同目录临时文件 rename 替换，仅适用于节点本地；`redis` 通过可选 `cache-redis` feature 提供 shared store。无本地静默回退。文件替换原子性遵循操作系统 rename 语义。
- 外部 driver 实现 `CacheDriver`/`CacheBackend`，于插件 `build` 中通过 `AppBuilder::extension_mut::<CacheDrivers>()` 注册。后端暂只承诺基础 get/set/remove/TTL，不提供原子操作、tag、SWR 或 singleflight。
- CLI 生成应用默认启用 `cache-redis` 并安装 `CachePlugin`。`openrust dev` 复用可达 loopback Redis；否则用 workspace `compose.ci.yaml` 启动 Redis。生产与 `serve` 不自动编排依赖。

验收入口和未覆盖边界见 [`memory/testing.md`](../memory/testing.md) 与 [ADR-014](../decisions/adr-014-cache-driver-and-store-contract.md)。
