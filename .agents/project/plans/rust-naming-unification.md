# Rust 命名统一开发计划

确认日期：2026-11-23。出处：用户在模型字段侧对话确认采纳 Rust 原生命名方案，并要求记录开发计划，交由后续 Agent 实施。

状态：用户于 2026-11-23 追加授权按 N-01 至 N-06 实施、分批本地提交，不推送。N-01 至 N-05 已完成本批验收，N-06 待实施；实际状态以[专项验收](../audits/rust-naming/README.md)为准，不以计划授权代替验收。

## 决定与边界

机制、职责、默认值、生命周期和可观察行为仍按固定上游基线核对；Rust 标识符采用原生命名。此决定替代此前“公开方法、参数和内部变量只要 Rust 能表达就保留上游大小写”的要求。品牌中性化单独按项目规范处理，不借本次迁移修改包身份或外部协议。

| 对象 | 规则 | 示例 |
| --- | --- | --- |
| 类型、trait、枚举、变体、类型参数 | UpperCamelCase | Application、ServiceProvider、NotFound、T |
| 函数、方法、变量、参数、字段、模块、宏 | snake_case | base_path、with_routing、get_locale、model_fields! |
| 常量、静态变量 | SCREAMING_SNAKE_CASE | DEFAULT_LOCALE、TABLE |
| Rust 源文件 | snake_case.rs；沿用既有模块布局 | service_provider.rs |
| crate 导入名 | snake_case | openrust_framework |
| Cargo 包名 | 沿用项目小写短横线约定 | openrust-framework |
| 环境变量、命令名、配置键 | 保持既定契约 | APP_NAME、install:api、app.locale |

缩略词遵循 Rust 习惯，例如类型使用 Http、Url、Uuid，方法使用 http、url、uuid；`Self`、`r#type` 等语言规定写法保留。表中规则只规定大小写，不授权重新设计接口或无必要地重命名语义，例如不要求把所有 get_* 改成另一套 getter。

Rust 标识符与字符串契约分开处理。JSON／Serde 字段、数据库列、队列载荷、缓存键、事件名称、容器服务键、URL、CLI 选项、模板表达式、供应商协议与 FFI 符号不得机械改名。结构体字段改名时用显式 Serde 映射等保持原格式；反射、动态分发或字符串映射到方法的地方逐项记录桥接。Blade 等既有表达式的名字是独立契约，不能仅因对应 Rust 方法改名就同步替换。

当前首版迁移直接统一本仓库公开 Rust API 与消费者，不长期保留两套大小写别名。若发现已发布包或外部消费者，先记录实际兼容影响，必要的兼容方案待确认；不得因此阻塞无关模块。许可证、来源和历史验收原貌保留。

## 实施顺序与完成条件

每批记录“待核对／实施中／验收通过／阻塞”，未验收不算完成。按实际依赖调整批内顺序，不能只改定义不接调用方。

| 批次 | 工作范围 | 闭合条件 |
| --- | --- | --- |
| N-01 清单与边界 | 清点全部 workspace crate、应用、扩展、辅助工具、宏、生成器与模板；检查大小写 lint 豁免及字符串桥接 | 按所属类型记录旧名、目标名、消费者、外部契约、例外与验收入口；检查大小写转换碰撞、关键字及同名方法 |
| N-02 基础装配 | Container、Contracts、Application、Provider、配置与路径接口 | 定义、trait 实现、Facade、注册、根应用、测试与示例一起迁移；应用和 Artisan 实际可运行 |
| N-03 请求和命令 | HTTP、路由、中间件、Session、认证授权、Artisan、安装器和生成器 | HTTP 与 CLI 成功／错误路径通过；新生成项目及生成源码使用目标命名并实际编译 |
| N-04 数据与基础服务 | 数据库、Schema、Eloquent、集合、分页、Resources、缓存、Redis、国际化、视图、日志和异常 | 公开消费者与受影响真实数据库／缓存验收通过；序列化、模板、数据库格式不变 |
| N-05 异步与扩展 | Bus、事件、队列、调度、邮件、通知、短信、存储、广播、WebSocket，以及所有 workspace 扩展包 | 真实 Worker、调度及受影响协议场景通过；扩展 trait、宏与可选 feature 实际编译，不能只检查默认配置 |
| N-06 文档与防回归 | 三语正文、代码示例、Rustdoc、入口、CI 和命名检查 | 全仓清单逐项裁决；当前代码示例无旧 Rust 调用，三语构建与链接检查通过；历史记录不改写 |

每批同步受影响文档和模板，N-06 负责全仓复核，不积压到最后才改文档。使用已有工具和简单清单，不另建审计平台。IDE 重构只能辅助，必须审查 diff；禁止全仓文本替换或顺手改业务逻辑。

## 验收与交付

- 使用项目构建守卫及现有验收命令。先运行格式、受影响 crate 的 check／Clippy／测试，再按最终组合运行 workspace 与适用 feature 检查；互斥 feature 分开验收，不盲目使用 all-features。
- 用实际公开入口编译消费者，覆盖 trait／Facade／宏生成代码、根应用和新生成项目。对受影响的 HTTP、Artisan、数据库、Worker、安装与发布路径实际运行；已有证据只有在源码组合与依赖未受影响时可复用。
- 命名 lint 为 non_snake_case、non_camel_case_types、non_upper_case_globals；移除为保留上游大小写添加的 allow／expect。CI 将命名告警作为失败；FFI、外部 trait 或协议确需例外时使用最小范围豁免，记录具体原因与证据，不全局关闭。
- 比较关键数据／输出格式，确认默认值、返回值、错误和执行时机未因改名发生变化。动态字符串接口与 Rust 方法分别验收。
- 三语文档、Rustdoc 和生成模板使用新 Rust 命名；必要的上游映射保存在内部清单，保留来源与许可。
- 每批保留源码基线、命令、日志、结果、差异、例外和剩余项；完成清理并记录空间后再开下一批。临时目录与产物全部遵守项目规范。
- 汇报实际完成范围，不以扫描无命中、编译通过或清单数量代替功能验收。任务完成且相关验证通过后按[Git 操作规范](../conventions.md#git-操作)单独本地提交，不推送；当次明确例外优先。

## 调研依据

- [Rust API Guidelines：Naming](https://rust-lang.github.io/api-guidelines/naming.html)：按对象种类采用大小写约定，宏采用 snake_case，缩略词按 Rust 单词处理。
- [rustc 命名 lint](https://doc.rust-lang.org/rustc/lints/listing/warn-by-default.html)：复用编译器的现有检查，不引入自建命名检查框架。
- [固定行为基线](laravel-rebuild-baseline.md)、[移植原则](../conventions.md#laravel-到-rust-的移植原则)与现有仓库消费者：行为对照保持，Rust 拼写按本计划迁移。
