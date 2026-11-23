# Rust 原生命名迁移验收

2026-11-23 用户授权：按 N-01 至 N-06 全仓实施、验收后分批本地提交，不推送。起点为 `8ae49738324ae7fcbae0196296d3bb3e137e7fe1`。保留用户环境配置、其他模型验收记录及原暂存的 Unicode／Zend 许可删除，不纳入命名提交。

| 单元 | 状态 | 实际范围及证据 |
| --- | --- | --- |
| N-01 | 验收通过 | 语法树盘点、workspace 清单、调用位置、字符串边界及碰撞检查；无运行代码改动 |
| N-02 | 待实施 | 容器、Contracts、Application、Provider、配置、路径及其实际消费者 |
| N-03 | 待实施 | HTTP、路由、认证、中间件、Session、命令、安装与生成源码 |
| N-04 | 待实施 | 数据库、集合、缓存、Redis、翻译、视图、日志和异常 |
| N-05 | 待实施 | 异步服务、协议和全部扩展 |
| N-06 | 待实施 | 全仓复核、三语、Rustdoc、CI、防回归及最终真实消费者 |

## N-01：盘点与边界

- `cargo metadata --no-deps --format-version 1` 确认实际 workspace、目标与 feature；清单位于 `target/tmp/rust-naming/workspace.json`。不凭目录中嵌套夹具的 Cargo.toml 数量判断 workspace 数量。
- `rust-analyzer parse --json` 读取 1,939 份正式应用、框架、扩展、工具及测试 Rust 文件，不改第三方补丁或历史源码。语法节点区分声明、标识符、字符串和注释，UTF-8 字节位置用于核对，不能按全文字符替换。
- 发现 6,416 处待核对声明、3,546 个待迁移拼写、31,286 处标识符位置、1,718 处字符串／注释桥接候选；这是候选计数，不是迁移完成率。定义及消费者存入 `registry.json`，原始 AST 摘要保留在 `target/tmp/rust-naming/inventory.json`。
- 按依赖及同名方法族分配 N-02 至 N-05。一个方法族跨 crate 的定义、trait、实现和消费者必须同批接线，不能留下同名 trait 不匹配。其余服务不借机改业务。
- 单列项目类型 `DB → Db`、`URL → Url`、`OAuthError → OauthError`、`OAuthParameters → OauthParameters`；外部导入不随同改写。`XProvider` 的单字母词不是连续缩略词，不改语义。
- 同作用域的既有 snake_case 与 camelCase 方法存在 51 处潜在碰撞，已记录文件、语法作用域、旧名和目标名。每批必须区分重复别名、私有实现和参数不同的操作，按职责合并或命名，不能批量制造重复定义。不同作用域的 Faker `VAT` 与 `vat` 不强行合并。
- Windows 的 AttachConsole、SetConsoleCtrlHandler、GenerateConsoleCtrlEvent 等 extern 声明保持链接符号，必要时限定局部 lint 例外，不能改坏 FFI。`None`、常量匹配、类型路径不会当作小写变量处理。
- Facade 关联常量 `cached` 单独迁移为 `CACHED`；数据库查询辅助方法 `cached()` 不受此常量迁移影响。

### 字符串与实际消费者边界

| 类别 | 保留内容 | 每批验收入口 |
| --- | --- | --- |
| Serde／资源输出／数据库 | 原 JSON 字段、枚举、数据库列和载荷键；字段改名核对显式 rename | 序列化往返与实际 HTTP／数据库／Worker |
| Blade／动态格式化 | currentLocale、getLocale、DB、URL 等既有模板表达式；仅后端 Rust 接线改名 | 真实视图编译、模板运行、语言与请求对象消费者 |
| 宏／生成器 | 宏参数中的真实 Rust 方法迁移；属性选项、字符串协议保持；Rust 源码字符串单独核对 | derive／trait／Facade 消费者、新生成项目及源码实际编译 |
| CLI／事件／控制键 | 命令、参数、退出码、事件字符串、缓存和容器服务键 | 实际 Artisan、HTTP、Worker、调度及协议消费者 |
| 测试／三语／Rustdoc | Rust 示例迁移；PHP／Blade／JS 等代码块按各自契约保留 | 定向测试、文档构建、真实浏览器与导出检查 |

### 工具核对

使用现有 Rust 解析器和编译器，不引入生产依赖；盘点脚本为 `.agents/project/tools/rust_naming_inventory.py`，不会自动编辑源码。迁移只使用核定的符号与语法位置；字符串候选需要单独裁决。完整计划见 `plans/rust-naming-unification.md`，历史记录保持原貌。

N-01 尚未开始服务代码迁移，不表示任何运行模块重新验收通过。后续批次将追加实际命令、结果、源组合、数据格式对照、异常及剩余项。临时产物均在 `target/tmp/rust-naming/`，按构建守卫管理；不删除依赖下载、数据卷、源码或原索引。
