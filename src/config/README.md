# 配置仓储读取

`ConfigRepository` 是各 Provider 共享的同一配置仓储；`section::<T>` 保留类型化读取。

| 方法 | 返回值（均为 `Result<…, ConfigError>`） | 行为 |
| --- | --- | --- |
| `get(key)` | `Option<Value>` | 缺失为 `None`；显式 null 为 `Some(Value::Null)` |
| `get_or(key, default)` | `Value` | 仅缺失使用默认值 |
| `has(key)` | `bool` | null 仍算存在 |
| `get_many(&[key])` | `Vec<(String, Value)>` | 缺失为 null |
| `get_many_with_defaults(&[(key, default)])` | `Vec<(String, Value)>` | 每键默认值；null 保留 |
| `all()` | `Value` | 已发布配置的独立快照 |

查询优先匹配完整字面键，再以点逐段遍历。`a.b` 字面键可覆盖同名嵌套路径的读取；`a.b.c` 不会在中途把 `b.c` 当完整键。数组支持 `items.0.name`，`00`、`+0` 不作为索引零；对象的 `01` 仍是有效命名键。空字符串查询空键，全部配置使用 `all()`。

批量操作只读取一次快照，按键首次出现的顺序返回；重复键取最后一次结果。每次操作完整解析当前来源，失败整体返回脱敏错误。读取不改变版本、默认值发布、overlay 或 freeze；注册期只读当前来源，Provider 默认值仍在原有发布时点生效。

成功返回的值与 `snapshot()` 一样可能包含秘密。调用方在日志、CLI 或 HTTP 输出前须使用适用脱敏机制。此读取簇的原始来源错误不显示来源提供的路径；类型化来源错误仅保留按 schema 层级核对的字段，来源提供的规范数字数组段显示为 `<index>`，动态 Map 名称显示为 `<entry>`。schema 自行校验数组时生成的索引仍可用于可信诊断；未知键和动态 Map 键不出现在错误路径中。这里的脱敏保证限于上述路径与原始来源错误，不能代替调用方对成功值及其他业务错误的审查。

业务代码可按固定白名单键读取，例如 `repository.get_or("app.name", serde_json::json!("应用"))?`；返回值是原始 JSON，调用方仍须判断类型。独立公开 API 调用与边界测试位于 [config-repository-reading consumer](../../tests/consumers/config-repository-reading/README.md)。生成器建立空 `modules/`，开发者可在自己模块的 Provider 中取得 `BootContext::config_repository()` 并把其克隆放入业务服务；当前生成器不会复制唯一示例模块。

当前子簇的接口与固定上游映射见 [ADR-086](../../.agents/project/decisions/adr-086-config-repository-reading.md)。唯一示例的固定业务读取见[模块路由](../../modules/example/routes/api.rs)与[服务](../../modules/example/services/config_reading.rs)；生成应用可显式安装上述 consumer 的 `ReadingProvider`。本分支已通过最小 feature 的具名单元、模块、consumer、生成项目和本地 TCP 验证；完整 feature 与主线组合仍待集成验收，不代表完整 Laravel Config 已验收。
