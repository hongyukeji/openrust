# ADR-014 缓存驱动、命名实例与公共契约

- 状态：Partially Superseded（标准应用默认值由 [ADR-015](adr-015-redis-default-calibration.md) 校准为 Redis；公共契约继续有效；基础驱动与 F4a tag generation 已实现，singleflight/SWR 继续按 todo 推进）
- 日期：2026-10-25
- 补充：ADR-009 能力驱动扩展原则；标准应用默认值以 ADR-015 的 Redis 决定为准。单机/集群部署边界仍以 ADR-013 为准。

## 背景

v1 需要让业务代码在合理范围内不依赖具体缓存后端，同时提供默认 Redis 的标准项目与显式 memory/file 选择，以及 named stores。能力落在根 `openrust::cache`，原独立空壳 package 已按 ADR-021 移除。Laravel Cache 的 default store、named stores 和 custom driver 是本决策的参考；V1 综合评估 Loco、Laravel、Goravel、OpenSpring，若依与芋道仅作为 V2 参考，保持 Rust 类型、显式依赖、Plugin 和 ADR-006 命名规则。

## 最小 Spec

- 在根 `openrust::cache` 实现规划中的 `Cache` 与 `CachePlugin`；不恢复独立空壳 package，不建平行缓存入口、通用基础设施 crate 或万能 driver trait。
- 一个 driver 是后端实现；一个 store 是有名称、driver 和配置的实例。应用有配置指定的 default store，也能按名称取得其它 store。
- v1 官方驱动为 Redis（标准应用默认）、进程内 memory 与 file（显式选择，本地开发/单机适用）；Redis 在库层通过 feature 按需组合；其它驱动可按公开注册契约扩展，不列入 v1 支持矩阵。
- 公共基础行为包括带类型的读写、删除、TTL、key namespace 和统一 serde 序列化；缓存未命中使用 `None`，序列化或 driver 故障返回 `Err`。
- Driver 扩展契约负责构建命名实例、解析其配置并实现基础 bytes/TTL 操作；所有 Plugin 的 `build` 阶段收集 driver 注册和能力需求，`CachePlugin::finish` 再解析配置、校验并构建实例。未知 driver、缺失 feature、重复 store 名或无效配置在启动时明确报错。
- 安装 `CachePlugin` 后若缺少 `[cache]` 或 default store 配置，启动时明确报配置错误，不隐式创建 memory store；标准模板显式配置 Redis default store 并启用 `cache-redis`。显式选择不存在的 store 是配置错误，不回退到 default store。
- 应用通过 `Inject<Cache>` 获取已配置的 default 句柄；`cache.store(name) -> Result<CacheStore>` 取得具名实例，所有实例共用相同的类型化操作 API。这里的“default”指配置选定的实例，不提供全局工厂或静态入口。
- 每个实例公开可检查的作用域与能力（进程内、节点本地或跨节点共享，以及真实支持的原子操作）；需要共享/原子的框架 Plugin 声明目标 store 名及所需能力，由 `CachePlugin` 校验那个已解析的实例，不满足时启动失败。driver 的静态能力在启动校验；外部服务连通性由 driver 初始化与 readiness 检查报告，不能用 capability 声明代替 Redis 可用性验证。
- 基础 `get_or_insert_with` 不承诺并发只执行一次；V1 另提供显式 singleflight，注明 store/key/类型隔离与进程范围，跨节点协调复用共享锁并声明租约边界。`increment`、条件写入和锁只有驱动实现真实原子原语时才可用，能力不支持时显式报错。memory/file 不提供跨进程或分布式锁。
- cache tag、SWR 与 singleflight 已批准为 V1 必交，详细契约和验收统一见 [补齐计划](../plans/v1-foundation-completion.md)。tag 明确失效与并发回填边界；SWR 明确新鲜/陈旧窗口、失败和有界刷新生命周期；不把陈旧缓存、tag 或请求合并用作权限、会话撤销及其他权威状态。缓存故障不得伪装成未命中或自动换 store。

## 决策

- 配置采用 `[cache] default_store = "default"` 和 `[cache.stores.<name>] driver = "memory|file|redis"` 的类型化模型；每个 store 可定义自己的 namespace 与驱动配置。环境覆盖遵循 ADR-006，Redis URL 使用 `REDIS_URL`，同键所选应用命名空间（默认 `APP_`）变量优先（ADR-016）。实际字段结构在 M5 Spec 中按当前配置 parser 与 Rust 类型再校准，不提前固定供应商参数。
- 业务注入 `Cache` 使用 default store；需要特定实例时通过 `Cache::store("name")` 显式选择并获得结果类型。`Cache` 保持唯一公共能力名，`store` 只表示命名实例。
- 缓存值使用 serde JSON 编解码，错误类型区分未命中、序列化失败、配置/能力错误与驱动 I/O/协议故障。每个 store 的 namespace 与逻辑 key 按驱动一致拼接；后端 key 不接受路径片段作为路径。
- memory 状态只在一个进程内共享。file 将逻辑 key 稳定散列为文件名，限制在配置根目录内；写入用同目录临时文件加原子替换，过期读取返回未命中，并提供有界清理入口；并发读写保证不暴露半文件，竞争写入按最后一次完整替换生效，不承诺多机共享、线性一致更新、原子计数或锁。
- Redis 在库层作为可选 feature 与真实外部 driver；标准生成项目显式启用该 feature 并默认配置 Redis。集群部署若某能力需要缓存跨节点共享，必须显式选择 Redis 等满足契约的共享 driver；缺少该后端时启动失败，不静默退化到 memory/file。普通非权威缓存仍可按应用明确配置使用本地 driver。
- 第三方 driver 通过 `CacheDriver` 注册入口构建 `CacheBackend` 实例；基础 trait 只承诺 get/set/remove/TTL 的 bytes 语义。批量 API 可由基础操作逐项执行，不承诺批量原子性；驱动提供的真正批量原语可在契约测试证明后作为可选能力暴露。原子操作由明确的可选能力接口表达，不要求每个驱动假装支持全部能力。公开 trait 的最终异步边界和配置表示须在 M5 实现 Spec 中与依赖选择一并确定，并由独立扩展示例验证无需修改 OpenRust。
- 所有内置 driver 跑同一基础契约测试；memory/file 用确定性单测，file 用临时目录验证 key 隔离、过期、替换、并发写和 I/O 故障；Redis 需真实服务集成验证。feature 矩阵覆盖默认、file、redis 和有效组合。

## M5 基础实现细化

- 基础 public API 已落在 `openrust::cache`：`Cache`、`CacheStore`、`CachePlugin`、`CacheDriver`、`CacheBackend`、`CacheDrivers` 与 `CacheDriverConfig`。driver backend 使用对象安全 async trait；扩展包在自己的 `Plugin::build` 中通过 `AppBuilder::extension_mut::<CacheDrivers>()` 注册，`CachePlugin::finish` 在所有插件 build 完成后读取配置并初始化。
- 配置用 `[cache] default_store` 与 `[cache.stores.<name>]`；driver 专属选项保留为扁平 serde JSON 字段映射。store/key 命名空间统一在业务 API 层组合。零 TTL 被拒绝；TTL 统一传 `Option<Duration>`，Redis 转为毫秒，file 使用 Unix 毫秒过期时间。
- 内置 driver 的作用域分别是 memory=`Process`、file=`NodeLocal`、Redis=`Shared`。file key 通过 SHA-256 映射为固定文件名，临时文件同目录 rename 替换；不提供多机共享、线性一致更新、锁或原子计数。memory/file 和 Redis 均只承诺基础 get/set/remove；原子能力与 F4 的 tag/SWR/singleflight 仍在后续功能簇。
- Redis crate 选用 `redis` 1.8.0，默认 features 关闭，仅启用 `connection-manager` 与 `tokio-rustls-comp`，通过可选 `cache-redis` 启用；初始化时建立连接并失败关闭，I/O 错误不转为 miss，敏感 DSN 不进入错误或 Debug。最低 Redis 版本仍待 v1 发布前兼容专项验证。
- `redis` 的 `xxhash-rust` 是非可选依赖，采用 OSI 批准的 Boost Software License 1.0；将 `BSL-1.0` 明确加入 `deny.toml` allowlist，不启用 Redis crate 的其他默认可选能力。
- 标准 CLI 生成项目安装 CachePlugin、启用 `cache-redis` 并配置 Redis default store。`openrust dev` 仅在默认 store 使用 loopback Redis 且 PING 不可达时启动仓库 Compose Redis；可达服务复用不重建，启动的服务保留供后续开发复用。`serve` 不代替开发服务编排。

## F4a tag 代际失效 contract

- 可选 backend 能力 `CacheTagBackend` 与基础 `CacheBackend` 分离；`CacheStore::tagged(tags)` 返回受同一 store 与 namespace 隔离的视图。普通 get/set 不受影响；file/custom backend 未实现该能力时 tagged API 返回稳定 unsupported 错误，不做本地替代。
- tag 名称为应用管理的稳定标识；输入 tag 集合排序、去重且有界。每个 tag 有持久 generation；命中键包含逻辑 key 与 tag generation，不遍历删除全部成员。Redis generation 不设置 TTL，避免元数据过期后 generation 回到旧值；file 明确不支持。
- tagged 写入需 atomic `set_if_generations_match`：失效与回填竞争时，旧 generation loader 不可写入当前可读键空间。Redis 使用单条 Lua 脚本比较全部 generation 并写入值；memory 在同一锁下比较并写入。多 tag 失效逐项原子，整体不保证跨 tag 全有或全无；调用失败时返回错误并说明可能部分失效。
- tag invalidation 不删除旧物理 key；调用者需为 tagged value 设置有限 TTL 以回收旧代 payload。tag generation 元数据随每个被使用的稳定 tag 持久存在，应用不得把高基数用户输入直接作为 tag。
- 基础 `CacheBackend` 新增默认“不支持”可选能力方法，保持已实现 custom driver 可编译；外部 driver 明确实现 `CacheTagBackend` 后才可使用 tag API。

## 参考与取舍

- Laravel 官方 Cache 文档：<https://laravel.com/docs/13.x/cache>，参考其默认 store、按名称选择 store 与注册 custom driver 的职责拆分。
- OpenRust 保留 `Cache` 服务注入和 `CachePlugin`，不用 PHP Facade；采用显式 `Result`、trait/注册、类型化配置和可检查能力边界。
- 该参考不作为缓存默认后端、具体方法语义、原子性或集群支持证据；这些由本 ADR、实际实现与测试定义。

## 影响与实施顺序

- ADR-009、ADR-013、上下文、栈、v1 计划与集群计划按 ADR-015 同步到标准应用 Redis 默认、显式 memory/file 与受校验的共享语义。
- 实现顺序以 `todo.md` 为准；本 ADR 规定 contract 和验收；基础 Cache 与 memory/file/Redis driver、F4a tag generation 已实现，后续高级能力仍按 todo 验收。
- M5 实施已复核 `CacheDriver` / `CacheBackend` 的异步边界、TTL 毫秒精度、file 路径与 rename 行为及可选原子 tag 能力；不弱化本 ADR 的错误区分和作用域边界。完整 Redis Cluster client 与多 key 跨 slot 能力不在本簇保证内。
