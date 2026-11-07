# OpenRust 框架

OpenRust 是 Laravel 的 Rust 实现；Laravel Modules 是默认集成的优秀第三方扩展。框架库公开路径为 `openrust::`，根应用通过统一 Container、Provider 和配置生命周期接入。

框架源码位于 `src/lib.rs`；宿主应用、bootstrap 和唯一综合示例保留在仓库根目录。默认启动在根执行 `cargo run`。

功能与尚未完成的对应以[用户指南](../../../docs/src/SUMMARY.md)为准；独立框架包拆分已由 `44632383` 完成限定验收；职责命名空间与应用入口已由 `b3f6117b` 完成限定验收；完整官方契约与全量 Laravel 能力仍按[命名空间对应计划](../../../.agents/project/plans/laravel-namespace-alignment.md)逐项验收。

模型属性状态通过同一 `openrust::database::eloquent::ModelAttributes` 提供原始基线、cast感知变化查询与显式快照同步。同步不等同数据库保存；完整Model生命周期仍在对应。公开用法与并发/错误边界见[模型基础](../../../docs/src/eloquent/getting-started.md#原始基线与变化记录)。

事件通过唯一 `EventsServiceProvider` 登记与 `Events` 派发。类所有者声明 `Event::NAME`；类与同名字符串使用同一个 exact/wildcard 选择器。`listen_reply` 和跨类型 `listen_named_reply` 返回保身份的 `DataValue`，普通派发收集响应，`until` 取得首个非 null。普通 false 只停止传播；业务取消由领域显式解释。立即派发返回 `Some`，完整接口投影、多参数、延迟/后提交与可变模型生命周期仍待对应，详见[事件指南](../../../docs/src/getting-started/source-application.md)。

`hashing` 的统一 `HashManager` 明文入口接受文本或原始字节，公开 `Hasher` 的明文参数为 `&[u8]`；已编码哈希和元信息仍为文本。需要先检查完整输入能力再生成时，用无参 `driver_for_operation()` 一次取得默认Manager链的最终具体叶，两步复用同一叶；普通自定义 `Hasher` 不必实现Manager hook。bcrypt/Argon的输入等价关系与自定义扩展方式见[哈希指南](../../../docs/src/security/hashing.md)。

模型默认写入可通过 `ModelAttributes::set_attribute_with_default_setter` 提供借用型调用级 setter；已登记的自定义 setter 优先，缺省分支自行完成转换，复用同一补丁与并发冲突检查。它不登记第二套访问器或存储，也不撤销用户 setter 的外部副作用。

`support::calendar::format_php_numeric_timestamp` 将有限 `f64` 按 PHP 六位 half-up 格式化；`parse_php_numeric_timestamp` 将 JSON 数字或数值字符串按 `U.u` 秒/微秒字段解析为固定 UTC 的 `CalendarInstant`。负数、精度及类型错误边界见[数值日期适配](../../../docs/src/getting-started/source-application.md#数值日期适配)，唯一模块在现有 input 路由展示公开调用。接口不改变 `Request::date`，也不表示完整 Model 日期或 JWT 已完成。
