# 安装器的 User 声明与隐藏字段识别

2026-11-24。承接[生成回归语法修复](syntax-regression-2026-11-24.md)之后的只读复核，处理 Fortify 隐藏字段和 Fortify／Passport 声明识别两处安装器缺口。本轮不恢复已取消的 GitHub 工作流，不调整其他包业务实现。

## 实现与保留边界

复用 framework 已有 proc_macro2／syn 依赖和语法定位经验，无新增依赖。旧版 v0.x 中未找到同路径安装源码修改辅助实现，沿用当前安装器的 SourceInstallation、规划后写入、锁与定制保留机制，不另造源码生成体系。

- `source_edit::UserModelSource` 以真实 Rust 语法解析目标文件：注释及字符串中的声明不算实际结构、derive 或 trait impl。Clone 定位到 User 的 derive／手写 impl，能力接入识别 User 的对应完整 trait 路径；其他模型的声明不算接入。新增 Clone 只支持已知 ModelState 包装，未知定制结构要求人工接入；不改写用户已有 Clone 或能力实现。
- Fortify 只向 `impl Model for User` 的 `metadata()` 实际返回静态 ModelMetadata 构造链中最后生效的 hidden 数组补缺字段。fillable、无关函数或静态项、被覆盖的 hidden 不作为有效声明。按 span 插入最小文本，保留原有字段、注释、方法及其他源码；支持原始字符串、尾逗号及已有部分隐藏字段。引用外部常量数组等无法安全定位的结构明确拒绝，不自动重构。
- Fortify 的源码规划在发布及替换前完成；无效语法或无法识别的定制结构不产生部分文件改写。保留重复安装、现有公开入口及原先安装资源归属。Passport 只更正模型能力存在性识别，保留既有 Provider 方法和 API guard 等接入规则。
- 这不是 Rust 名称解析器、宏展开器或 cfg 求值器，遵循当前安装模板的完整 trait 路径和明确元数据结构。无法证明的自定义布局不视为自动安装成功。

## 验收设计

- 原生 Rust 测试覆盖错放 fillable、未返回静态项和被后续覆盖的 hidden；注释／字符串／其他模型中的伪声明；组合 derive 和手写 Clone 保留；部分原始字符串隐藏字段、尾逗号、重复调用；无效语法与不支持的定制拒绝。
- 生成应用在真实安装之前将双因素字段放入 User.fillable，并加入伪 Clone／Passport／Fortify 声明注释。执行实际 Sanctum、Passport、Fortify 安装和重复安装，仍以独立语法断言及原生编译核对产物，保留 User 方法、自定义 stub、其他模型和 Action 定制。
- 同一应用对无法定位的 hidden 和无效语法分别执行真实 Fortify 安装失败反例，逐文件确认 app、config、bootstrap、迁移源码无部分改写；恢复原文件后再次真实安装，要求 User 完全不变。
- 最终由生成应用的真实 HTTP 与 Artisan 消费 User 和生成模型，验证非空敏感字段隐藏／错位反例、数据库创建／验证／回读／删除、迁移回滚及严格 Clippy。

## 本轮执行结果

基线 `ba6d7b32018d32badfc67a1af8f088fedea9db06` 加本批六个代码／测试文件，在项目 `target/tmp/installer-model-syntax-20261124/source` 隔离执行，其他协作者的在途改动未纳入。所有 Cargo 与生成应用子进程通过 `tests/build_guard.py` 使用 Rust 1.94.0、项目内 TMPDIR 和独占 Cargo target，保持正常增量与并发。

在同一 guard 环境、隔离 source 中依次执行：

```sh
cargo test --locked -p openrust-framework --test installer_user_model
python3 -B tests/generated_model_layering_contract_test.py
cargo fmt --all --check
cargo clippy --locked -p openrust-framework --test installer_user_model -- -D warnings
cargo clippy --locked -p openrust-fortify --lib -- -D warnings
```

真实生成应用通过独占 MySQL 入口执行：

```sh
python3 -B tests/build_guard.py --workflow installer-model-syntax-20261124 -- python3 -B tests/mysql_acceptance.py -- python3 -B target/tmp/installer-model-syntax-20261124/source/tests/generated_model_layering.py
```

- 新增原生 Rust 测试4项、既有 Python 契约单测11项通过，无忽略项。源码格式与两个受影响 crate 的定向严格 Clippy 通过。最初 Clippy 指出可合并的嵌套条件，修正后重跑通过，未放宽 lint。
- 独占 MySQL 8.4.13 中，默认真实 HTTP `/up`、`/`，make:model 普通／字段／具名连接／自定义 stub、已有模型拒绝覆盖，Sanctum／Passport／Fortify 首次和重复安装均通过。带注释伪声明、fillable 错位字段的实际 User 最终获得真实能力和正确 hidden；安装前后独立语法断言及定制保留检查通过。
- 真实 `fortify:install` 对不支持的 hidden 表达式及无效语法均按预期原因返回失败；应用源码文件集合和全部内容保持不变。恢复后再次安装，User 完全不变。
- 真实 Artisan 迁移、状态、路由查询及 `layering:probe`，与真实 TCP HTTP `/layering-probe` 均通过；实际完成 User 创建、验证、回读及删除。消费者 JSON 同时返回 `sensitive_hidden: true` 和 `misplaced_hidden_rejected: true`，匿名 `/api/user` 返回401。

生成应用 `cargo fmt --all` 后的独立 `cargo fmt --all --check`、`cargo clippy --locked --features sanctum,passport,fortify --bin main -- -D warnings` 及包迁移回滚全部通过。本轮不把 Python 编排命令数计为 Rust 测试数，也未新增 SQLite／PostgreSQL、完整认证／Passkeys 协议的验收结论。

日志位于本工作流 `parser-tests.log`、`contract-tests.log`、`fmt.log`、`framework-clippy.log`、`fortify-clippy.log` 及 `generated-layering-*.log`，初始 Clippy 失败日志单独保留。本轮开始时磁盘触及32 GB停线，guard 安全清理编译产物，释放62,857,183,232字节，清理后可用94,831,042,560字节；源码、日志、依赖下载、数据库与数据卷保留。

收尾已回收本批独占 MySQL 容器，保留开发数据库及数据卷。移除本批隔离源码、生成应用（含测试密钥）、临时运行目录和一次性编排脚本；保留日志与可复用编译缓存。只本地提交本批实现、测试和记录，不推送。
