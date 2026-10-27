# ADR-039 整套源码交付与内部包中性命名

> 替代范围见 [ADR-070](adr-070-laravel-first-rebuild.md#替代范围)；本文旧目录/命名/默认值不阻止全面重写。代码是否已迁移以当前实现和 todo.md 为准。

- 状态：Partially Superseded（目标设计由 ADR-070 重新开放；未替代的行为/验收事实保留）
- 日期：2026-10-26
- 更新：2026-10-27（四个工具 package 统一收纳至 `tools/`，Cargo package 名保持不变）
- 替代：ADR-005 的发布包命名与 crates.io 分发前提、ADR-021 的工具包保留品牌前缀及默认独立生成项目方案；其余许可证、MSRV、模块边界和目录职责继续有效。

## 背景

OpenRust 的使用方式是获取整套框架源码，在同一工程基础上开发业务项目。框架不作为独立 Cargo 包发布到 crates.io，不能再以注册表名称辨识度为理由给内部工具和应用统一添加 `openrust-` 前缀。

## 决策

- 默认交付整套源码 workspace，开发者在 `apps/<name>/` 编写业务，框架能力继续位于根 `src/`，配置代码位于 `src/config/`。业务通过公开接口扩展，不因源码随项目交付就要求改核心实现。
- 源码 workspace 的根 `server` 也允许开发者把同仓业务 Rust modules 放在 `apps/` 并由根宿主显式编译、注册；这与生成器创建的 `apps/<name>/` 独立 package 是两种不同模式，具体见 [ADR-063](adr-063-explicit-app-module-registry.md)。
- 根 package、公共路径 `openrust::...` 与 CLI 命令 `openrust` 保留产品身份；内部 package 使用职责名称，与所在目录末级名称一致。后续新增内部工具和应用遵守同一规则，不强制品牌前缀。

| 路径 | 迁移前 package | 当前 package | 目标二进制/库名称 |
| --- | --- | --- | --- |
| 根目录 | `openrust` | `openrust` | `openrust` |
| `tools/cli/` | `openrust-cli` | `cli` | 二进制 `openrust` |
| `tools/macros/` | `openrust-macros` | `macros` | proc-macro `macros` |
| `tools/testing/` | `testing` | `testing` | 库 `testing`，避免使用 `test` |
| `tools/performance/` | `performance` | `performance` | 性能基准 package |
| `apps/server/`（历史路径） | `openrust-server` | `server` | 二进制 `server`；现由 ADR-062 迁入根 package |

- 所有 workspace package 和生成的应用、migration package 显式设置 `publish = false`。不再预留 crates.io 名称、发布占位包或以 `cargo install openrust-cli` 作为使用入口；不引入发布时改名、双 manifest 或注册表兼容包装。
- 内部依赖使用 workspace/path，随整套源码解析，不使用内部包的注册表版本依赖，不硬编码开发机绝对路径。第三方 Rust 依赖仍正常使用 Cargo 注册表与锁文件；本决定不要求离线 vendor 全部依赖。
- 保留必要编译边界：`macros` 是独立 proc-macro；`testing` 单向依赖框架，仅作应用测试依赖；`cli` 承载命令与模板。根框架不反向依赖测试工具。
- 原 `apps/server` 的集成宿主职责保留；宿主 package 与代码路径按 ADR-062 迁入根 package，已有启动、feature、双库和进程验收不能丢失。综合用户示例位于 `apps/example/`，独立下游与真实拓扑验收位于 `tests/consumers/`，框架集成测试位于根 `tests/`；目录约定见 ADR-064。
- 取消独立包分发不取消框架版本、源码升级与兼容性说明；tag、源码 release、推送仍按 `AGENTS.md` 的独立授权边界处理。API 升级可对比已有源码基线，不要求存在 crates.io 发布版本。

## 实现 Spec 与顺序

1. 先保护当前 Agent 的 WIP；闭合当前可安全提交的最小簇后，优先执行本迁移，再领取其他功能。若旧簇受外部服务阻塞，明确登记并保留后切换，不无限等待，也不并行改同一批文件。
2. 同步五个 manifest、workspace dependencies、Cargo.lock、`macros::resource` re-export、`testing::` 调用、server 二进制与 `CARGO_BIN_EXE_server`、CI/脚本/文档命令；保持根 `openrust::...` 公共路径与宏展开目标。
3. `openrust new <name>` 默认在框架 workspace 的 `apps/<name>/` 生成应用，纳入同一 workspace，应用和 migration 使用根 workspace 的本地框架依赖，测试使用 `testing`。migration package 用应用名区分，避免多应用冲突；不重复创建嵌套 workspace。
4. 基于 Cargo 元数据实现框架工作区和目标应用识别；当前仅有 manifest 存在检查，不视为已有发现机制。新增成员须保留已有 manifest 内容；重复名称、已有目标目录、非框架工作区和不支持的外部目标应在写入前明确失败。既有外部路径参数不再生成缺失源码的半成品；完整源码导出工具不属于本迁移。
5. 生成器的 auth、双库、UUID、软删除、邮件、短信、加密及 migration 模板一起更新。特别检查按旧 `version = "0.0.0"` 字符串匹配的 feature 修改逻辑与快照，不只改静态模板。
6. 默认从源码运行 `cargo run -p cli -- …`；进入 `apps/<name>` 后 `dev/serve` 与运行期代理须明确启动该应用，不能误启动 CLI 或 `server`。配置按目标应用定位，避免根 `.cargo/config.toml` 中宿主配置覆盖业务应用；保留显式环境配置的优先级。
7. 同步 README、各工具事实、测试入口、当前计划与生成的 Agent 指引；只在迁移真正通过后将旧包名命令替换为新的当前可用命令，历史验收记录不改造成新命令已执行的证据。

## 验收

- `cargo metadata` 验证包名映射、workspace/path 依赖及所有 package 的禁用发布设置；比较迁移前后测试 targets/用例和 feature 清单，允许包身份变化，不允许丢测试。
- 执行 fmt、workspace clippy/test/doc、`cargo deny check` 和受影响 feature 检查；宏通过 `openrust` 门面使用，应用通过公开 API 接入，测试工具无反向依赖。
- 在 `/tmp` 干净源码副本中生成默认、PostgreSQL、MySQL 应用并直接 check/test；认证与 UUID/软删除等受影响产物按原能力验收，不再人工补 `[patch.crates-io]`，不依赖本机全局安装或原 checkout 绝对路径。
- 同一 workspace 至少生成两个应用，验证 app/migration 包名不冲突、各自启动和配置不串用；验证已有目录与无效目标失败时无半成品。
- PostgreSQL 18/MySQL 8.4 数据、认证、队列及相关进程测试实际运行；先复用现有合适服务。真实服务不可用时如实登记阻塞，编译或跳过不能视为通过。
- 更新全部有效文档/CI 命令和旧引用扫描后提交；单机/集群、性能与其他 v1 能力仍按原计划验收。

实现验收记录：包名、发布禁用、路径依赖、CLI 生成/启动、双应用配置隔离、认证与加密生成项目均已在干净源码副本验证；PostgreSQL 18 与 MySQL 8.4 的真实数据库及队列/调度相关测试通过。当前 workspace 门禁与干净副本证据见 `.agents/project/memory/testing.md`。完整 v1 尚未完成。
