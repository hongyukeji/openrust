# ADR-014 缓存驱动、命名实例与公共契约

- 状态：Partially Superseded（标准应用默认值由 [ADR-015](adr-015-redis-default-calibration.md) 校准为 Redis；其余公共 contract 有效，驱动实现待 M5）
- 日期：2026-10-25
- 补充：ADR-009 能力驱动扩展原则；标准应用默认值以 ADR-015 的 Redis 决定为准。单机/集群部署边界仍以 ADR-013 为准。

## 背景

v1 需要让业务代码在合理范围内不依赖具体缓存后端，同时提供默认 Redis 的标准项目与显式 memory/file 选择，以及 named stores。能力落在根 `openrust::cache`，原独立空壳 package 已按 ADR-021 移除。Laravel Cache 的 default store、named stores 和 custom driver 是本决策的参考；具体实现按六个参考框架统一评估，保持 Rust 类型、显式依赖、Plugin 和 ADR-006 命名规则。

## 最小 Spec

- 只扩展现有 `openrust-cache`、`Cache` 与 `CachePlugin`；不建平行缓存入口、通用基础设施 crate 或万能 driver trait。
- 一个 driver 是后端实现；一个 store 是有名称、driver 和配置的实例。应用有配置指定的 default store，也能按名称取得其它 store。
- v1 官方驱动为 Redis（标准应用默认）、进程内 memory 与 file（显式选择，本地开发/单机适用）；Redis 在库层通过 feature 按需组合；其它驱动可按公开注册契约扩展，不列入 v1 支持矩阵。
- 公共基础行为包括带类型的读写、删除、TTL、key namespace 和统一 serde 序列化；缓存未命中使用 `None`，序列化或 driver 故障返回 `Err`。
- Driver 扩展契约负责构建命名实例、解析其配置并实现基础 bytes/TTL 操作；所有 Plugin 的 `build` 阶段收集 driver 注册和能力需求，`CachePlugin::finish` 再解析配置、校验并构建实例。未知 driver、缺失 feature、重复 store 名或无效配置在启动时明确报错。
- 安装 `CachePlugin` 后若缺少 `[cache]` 或 default store 配置，启动时明确报配置错误，不隐式创建 memory store；标准模板显式配置 Redis default store 并启用 `cache-redis`。显式选择不存在的 store 是配置错误，不回退到 default store。
- 应用通过 `Inject<Cache>` 获取已配置的 default 句柄；`cache.store(name) -> Result<CacheStore>` 取得具名实例，所有实例共用相同的类型化操作 API。这里的“default”指配置选定的实例，不提供全局工厂或静态入口。
- 每个实例公开可检查的作用域与能力（进程内、节点本地或跨节点共享，以及真实支持的原子操作）；需要共享/原子的框架 Plugin 声明目标 store 名及所需能力，由 `CachePlugin` 校验那个已解析的实例，不满足时启动失败。driver 的静态能力在启动校验；外部服务连通性由 driver 初始化与 readiness 检查报告，不能用 capability 声明代替 Redis 可用性验证。
- `get_or_insert_with` 只提供读取未命中后的加载与写入便利，不承诺并发只执行一次；若将来支持单航班，必须注明进程范围。`increment`、条件写入和锁只有驱动实现真实原子原语时才可用，能力不支持时显式返回不支持错误。memory/file 不提供跨进程或分布式锁。
- cache tags 不纳入本 contract。缓存故障不得静默伪装成未命中或自动换到另一个 store。

## 决策

- 配置采用 `[cache] default_store = "default"` 和 `[cache.stores.<name>] driver = "memory|file|redis"` 的类型化模型；每个 store 可定义自己的 namespace 与驱动配置。环境覆盖遵循 ADR-006，Redis URL 使用 `REDIS_URL`，同键所选应用命名空间（默认 `APP_`）变量优先（ADR-016）。实际字段结构在 M5 Spec 中按当前配置 parser 与 Rust 类型再校准，不提前固定供应商参数。
- 业务注入 `Cache` 使用 default store；需要特定实例时通过 `Cache::store("name")` 显式选择并获得结果类型。`Cache` 保持唯一公共能力名，`store` 只表示命名实例。
- 缓存值使用 serde JSON 编解码，错误类型区分未命中、序列化失败、配置/能力错误与驱动 I/O/协议故障。每个 store 的 namespace 与逻辑 key 按驱动一致拼接；后端 key 不接受路径片段作为路径。
- memory 状态只在一个进程内共享。file 将逻辑 key 稳定散列为文件名，限制在配置根目录内；写入用同目录临时文件加原子替换，过期读取返回未命中，并提供有界清理入口；并发读写保证不暴露半文件，竞争写入按最后一次完整替换生效，不承诺多机共享、线性一致更新、原子计数或锁。
- Redis 在库层作为可选 feature 与真实外部 driver；标准生成项目显式启用该 feature 并默认配置 Redis。集群部署若某能力需要缓存跨节点共享，必须显式选择 Redis 等满足契约的共享 driver；缺少该后端时启动失败，不静默退化到 memory/file。普通非权威缓存仍可按应用明确配置使用本地 driver。
- 第三方 driver 通过 `CacheDriver` 注册入口构建 `CacheBackend` 实例；基础 trait 只承诺 get/set/remove/TTL 的 bytes 语义。批量 API 可由基础操作逐项执行，不承诺批量原子性；驱动提供的真正批量原语可在契约测试证明后作为可选能力暴露。原子操作由明确的可选能力接口表达，不要求每个驱动假装支持全部能力。公开 trait 的最终异步边界和配置表示须在 M5 实现 Spec 中与依赖选择一并确定，并由独立扩展示例验证无需修改 OpenRust。
- 所有内置 driver 跑同一基础契约测试；memory/file 用确定性单测，file 用临时目录验证 key 隔离、过期、替换、并发写和 I/O 故障；Redis 需真实服务集成验证。feature 矩阵覆盖默认、file、redis 和有效组合。

## 参考与取舍

- Laravel 官方 Cache 文档：<https://laravel.com/docs/13.x/cache>，参考其默认 store、按名称选择 store 与注册 custom driver 的职责拆分。
- OpenRust 保留 `Cache` 服务注入和 `CachePlugin`，不用 PHP Facade；采用显式 `Result`、trait/注册、类型化配置和可检查能力边界。
- 该参考不作为缓存默认后端、具体方法语义、原子性或集群支持证据；这些由本 ADR、实际实现与测试定义。

## 影响与实施顺序

- ADR-009、ADR-013、上下文、栈、v1 计划与集群计划按 ADR-015 同步到标准应用 Redis 默认、显式 memory/file 与受校验的共享语义。
- 实现顺序以 `todo.md` 为准；本 ADR 规定 contract 和验收，不表示 Cache、memory/file/Redis driver 已实现。
- M5 实施前重审 `CacheDriver` / `CacheBackend` 的实际 trait 形式、TTL 精度/零值、file 清理触发方式、各后端原子能力，以及 cluster capability 要求；不弱化本 ADR 的错误区分和作用域边界。
