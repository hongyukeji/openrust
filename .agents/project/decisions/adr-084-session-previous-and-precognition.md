# ADR-084：Session previous 与请求级 Precognition

状态：接受，限定实现与适用验收已完成；完整规则集/FormRequest仍在后继范围。固定基线为 Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`。

## 固定来源

[29份源码/测试摘要清单](../../../tests/http/fixtures/laravel-precognition/source-manifest.json)逐文件核对内容的 Git blob SHA1、字节数与官方 Git tree 条目一致，并记录 SHA256；仅证明内容来源，不表示执行过上游或 Rust 行为测试。核心契约来自固定版 [StartSession](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Session/Middleware/StartSession.php)、[Precognition middleware](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Http/Middleware/HandlePrecognitiveRequests.php)和[集成测试](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Integration/Routing/PrecognitionTest.php)。

字段规则值语义另对应 [ValidatesAttributes](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Validation/Concerns/ValidatesAttributes.php)；命名 middleware 排序对应 [SortedMiddleware](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Routing/SortedMiddleware.php)及其[排序测试](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Routing/RoutingSortedMiddlewareTest.php)。三项均纳入同一固定提交的来源摘要，不扩大本簇行为验收结论。

## 决策

- 沿现有 Session、Request、Container/Provider、受管理路由与 validation 领域实现，不另建 Hooks、解析器或路由内核。
- previous 使用普通 Session 点路径 `_previous.url` 与 `_previous.route`；公开原语保留现有 `Value`、缺失/null、记录预算和写入状态约束。
- Session 下游返回后，仅真实 GET Route、非 ajax/prefetch/confirmed 时记录完整 URL 与可空路由名，不按响应状态筛选；HEAD 不记录。
- `Precognition: true` 的精确值只表示 attempting；可选 middleware 建立请求私有、克隆共享的 confirmed 状态，不能由客户端同名头直接设置。
- confirmed 仍启动 Session、执行 GC、处理 Cookie，但不执行自动 driver.save；请求取消与失败不得偷偷后台保存，Cookie/错误清理沿既有安全约束。
- 受管理 Handler 在泛型擦除前复用 Axum 参数提取顺序；confirmed 时按顺序提取参数并验证，全部成功后返回空 204 和成功头，绝不调用业务函数；`Validate-Only` 的验证提取器可先行返回（见下条），因此不能保证其后任意提取器执行。原生 Router/Handler 逃生入口不虚称受管理覆盖。
- 普通与预检沿同一字段规则声明执行；先筛既有展开规则再运行，未选自定义规则不执行；仅提交选中字段不要求构造完整 DTO，不采用事后过滤错误。
- Validate-Only 缺失与存在空值不同；模式按逗号分隔且不 trim，星号只匹配非空单段。成功验证提前终止与默认 dispatcher 最终终止分别验证。
- Rust 参数提取按声明顺序且拒绝即返回；这是访问控制的明确边界。认证/授权须在路由 middleware 或 `Validated` 之前完成，禁止依赖其后提取器阻止字段预检结果。唯一模块采用 middleware 黄金路径；现有 router 回归分别证明先执行 middleware 拒绝，以及成功字段筛选跳过后置拒绝提取器，不能把后者称为已授权。
- middleware 普通响应亦追加 Vary，confirmed 响应追加 Precognition；不能因状态码为 204 就推断成功。认证/验证错误和并发隔离必须保持真实行为。
- middleware 优先级、规则执行细节及 Rust 提取拒绝类型在集成中核对固定源码与测试；未解决的行为不关闭，不将完整 FormRequest、URI 对象或官方前端包视为已交付。

## 验收

固定源码与测试逐项对应，覆盖真实路由/两请求持久化、匿名名称、错误/取消/保存失败、按字段规则与无业务副作用、请求隔离；同步唯一模块、生成应用、文档，执行完整适用门禁、PG18/MySQL8.4 与真实网络验收，再进行不同模型审查和限定本地提交。当前审计缓存仅为来源证据，不是行为通过记录。

## Rust 接入边界

- 以请求私有状态选择分派，代替 PHP 请求期间临时重绑应用级 dispatcher，避免并发请求污染；不增加第二容器。
- 普通强类型 JSON/Query 保留既有反序列化失败边界；字段预检则在构造 DTO 前执行选择规则。Laravel 字段规则的值语义与 Rust DTO 的类型约束分别验证，不把后者冒称上游默认。
- 规则读取原始、经现管线规范化的字段；serde 默认值在普通 DTO 构造时生效，不表示该缺失原始字段参与了规则执行。跨协议 `validate(T)` 对已构造值执行同源规则。
- 路由优先级以公开 alias 身份登记；内置 precognitive 仅在宿主显式挂载时运行，应用可覆盖同名 alias/group。全局 layer、原生逃生入口及任意类型层次的完整优先级对应不因本子范围自动关闭。
