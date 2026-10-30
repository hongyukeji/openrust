# ADR-086：同一配置仓储的原始读取

状态：限定实现，尚待总控组合验证与接入。

## Spec

1. 固定 Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的 `src/Illuminate/Config/Repository.php`、`src/Illuminate/Collections/Arr.php` 与 `tests/Config/RepositoryTest.php` 为来源；派发中的 `ConfigRepositoryTest.php` 路径不存在，以实际树更正。
2. 在现有 `ConfigRepository` 增加 `get(&str) -> Result<Option<Value>, ConfigError>`、`get_or(&str, Value) -> Result<Value, ConfigError>`、`has(&str) -> Result<bool, ConfigError>`。
3. 增加 `get_many(&[&str]) -> Result<Vec<(String, Value)>, ConfigError>` 与 `get_many_with_defaults(&[(&str, Value)]) -> Result<Vec<(String, Value)>, ConfigError>`；无默认的缺失值为 null，显式 null 不用默认。
4. 批量保留首次键出现顺序，重复键以后一次结果覆盖，符合 PHP 关联数组；Vec 避免 JSON map 排序改变输入顺序。
5. `all() -> Result<Value, ConfigError>` 返回当前已发布独立快照；以独立方法适配上游 null key 返回全部，不把空字符串当 null。
6. 完整字面键优先，然后按点逐段查找；不对中间剩余路径重新作字面匹配。支持空键与规范数字索引；`01` 不等于数组索引 1。
7. 复用 `snapshot()` 的原子 Figment 克隆及既有 `safe_source_error`；原始读取错误忽略来源提供的路径，类型化读取仅保留 schema 声明的诊断字段。不以 Figment dotted lookup 实现 Laravel 查询。本基线 support 无公开 Arr，故只加仓储私有值查找函数，不建立第二 Arr。
8. 每次批量操作只取一次快照并完整反序列化，来源失败整体返回脱敏错误，不返回部分结果；成功值含秘密，输出由调用方脱敏。
9. 不改 env、配置来源、默认/合并结果、版本、freeze 与 Provider 生命周期；仅收敛相邻 schema 与类型化错误的路径显示；不增依赖，不实现动态写入、typed helper 或 collection。
10. 单元覆盖 null/缺失/false/0/空、literal dotted key、数字索引、批量默认与重复、快照独立、失败脱敏及并发同快照；分支最小 feature 具名门禁已执行，完整 feature 组合仍待总控集成验收。
11. 唯一示例已接入固定 `example.greeting` 业务读取；独立 consumer 的公开 Provider fixture 已在新生成应用安装并通过具名测试与本地 TCP。此子簇不关闭完整 Config 或 L005。

## 安全诊断适配

原始读取与相邻的类型化配置校验共用错误边界。schema 校验只在错误路径显示已声明对象字段；未知对象键停留在已知父路径，Map 动态名称以固定 `<entry>` 表示，Sensitive 包装层同样执行该规则。类型化来源路径按真实 schema 层级逐段核对；来源提供的规范数字数组段也只显示固定 `<index>`，非规范数组段在已知父路径停止，来源的未知片段不回显。schema 自行校验数组时生成的索引仍可用于可信诊断。此诊断收敛是 OpenRust 的安全增强，不宣称 Laravel 默认错误文本等同。成功读取的原始值仍由调用方负责控制输出。

## 固定来源

- [Repository](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Config/Repository.php)
- [Arr](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Collections/Arr.php)
- [RepositoryTest](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Config/RepositoryTest.php)
