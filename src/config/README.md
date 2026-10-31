# 配置仓储原始读写

`ConfigRepository` 是各 Provider 共享的同一配置仓储；`section::<T>` 保留类型化读取。

| 方法 | 返回值（均为 `Result<…, ConfigError>`） | 行为 |
| --- | --- | --- |
| `get(key)` | `Option<Value>` | 缺失为 `None`；显式 null 为 `Some(Value::Null)` |
| `get_or(key, default)` | `Value` | 仅缺失使用默认值 |
| `has(key)` | `bool` | null 仍算存在 |
| `get_many(&[key])` | `Vec<(String, Value)>` | 缺失为 null |
| `get_many_with_defaults(&[(key, default)])` | `Vec<(String, Value)>` | 每键默认值；null 保留 |
| `all()` | `Value` | 已发布配置的独立快照 |
| `set(key, value)` | `()` | 按点路径替换尾项并发布一个版本 |
| `set_many([(key, value), …])` | `()` | 按迭代顺序处理，整批原子发布一个版本；空批次不变 |

查询优先匹配完整字面键，再以点逐段遍历。`a.b` 字面键可覆盖同名嵌套路径的读取；`a.b.c` 不会在中途把 `b.c` 当完整键。数组支持 `items.0.name`，`00`、`+0` 不作为索引零；对象的 `01` 仍是有效命名键。空字符串查询空键，全部配置使用 `all()`。

批量操作只读取同一已发布版本，按键首次出现的顺序返回；重复键取最后一次结果。原始值尚未发布时完整解析当前来源，解析失败整体返回脱敏错误；已有原始值时读取该版本的独立快照。读取不改变版本、默认值发布、overlay 或 freeze；注册期只读当前来源，Provider 默认值仍在原有发布时点生效。

写入遵循固定 Laravel `Repository::set` / `Arr::set`：即使根上已有字面 `a.b`，`set("a.b", value)` 仍写入嵌套 `a → b`，而 `get("a.b")` 继续优先读完整字面键。空键和连续点号也是可写的数组键；规范数字键可形成列表，稀疏数字键和数字、字符串混合键保留为有序对象。覆盖已有键不移动其位置，尾项整体替换；中途遇到 scalar 或 null 时创建数组。`set_many` 按输入迭代顺序执行，失败时原始值、Figment 类型化投影与版本均保持旧状态。

核心原始写入在 Register、Boot、Frozen 阶段均可调用。Register/Boot 写入仍受后续启动 schema 校验约束；Frozen 后的写入允许任意可表达的 `serde_json::Value`，后续类型化读取不匹配时返回安全错误。注册、boot overlay 和 freeze 的阶段限制保持不变。`App::config_repository()`、`AppState::config_repository()` 与 Provider 返回同一共享句柄；`App::section<T>` 读取最新仓储。`App::config()`、`AppState::config()`、`App::figment()` 和已经解析的服务保留启动快照，不因内存写入自动重建。`clone()` 共享服务句柄；`all()` 返回独立值快照。

运行期写入值与后续启动阶段的原始键序保存在同一仓储状态内；Figment 投影和来源元数据随同一版本原子发布。现有 `ConfigRepository::new(Figment)` 入口接收的 Figment 字典已经按键排序，无法恢复配置来源最初的对象键序；类型化投影的 map 顺序也受 Figment 字典表示约束。要完整保留来源初始顺序，后继须从 `ConfigLoader::figment()` 形成 Figment 之前传入有序来源值。固定 PHP 8.5.0 的值、键序、列表/混合/稀疏形态见 [golden fixture](../../tests/fixtures/config_runtime_set.json)；PHP 对象、Closure 和资源不属于 `serde_json::Value` 可表达范围，仍是公开差异。

此写入仅供可信应用代码使用，保存在当前进程内；它不写文件或数据库、不跨节点传播，也不刷新 DB、Redis、邮件等已建资源。管理员 Settings、授权、密钥和待重启策略见 [ADR-080](../../.agents/project/decisions/adr-080-runtime-configuration-scheduling.md)，不由此原始写入接口代替。

成功返回的值与 `snapshot()` 一样可能包含秘密。调用方在日志、CLI 或 HTTP 输出前须使用适用脱敏机制。此读取簇的原始来源错误不显示来源提供的路径；类型化来源错误仅保留按 schema 层级核对的字段，来源提供的规范数字数组段显示为 `<index>`，动态 Map 名称显示为 `<entry>`。schema 自行校验数组时生成的索引仍可用于可信诊断；未知键和动态 Map 键不出现在错误路径中。这里的脱敏保证限于上述路径与原始来源错误，不能代替调用方对成功值及其他业务错误的审查。

业务代码可按固定白名单键读取，例如 `repository.get_or("app.name", serde_json::json!("应用"))?`；返回值是原始 JSON，调用方仍须判断类型。独立公开 API 调用与边界测试位于 [config-repository-reading consumer](../../tests/consumers/config-repository-reading/README.md)。生成器建立空 `modules/`，开发者可在自己模块的 Provider 中取得 `BootContext::config_repository()` 并把其克隆放入业务服务；当前生成器不会复制唯一示例模块。

当前接口与固定上游映射见 [ADR-086](../../.agents/project/decisions/adr-086-config-repository-reading.md)。唯一示例的固定业务入口见[模块路由](../../modules/example/routes/api.rs)与[服务](../../modules/example/services/config_reading.rs)；生成应用可显式安装上述 consumer 的 Provider。读取子簇的既有验收不代替本轮写入、应用接线及真实请求验证，也不代表完整 Laravel Config 已验收。
