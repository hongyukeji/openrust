# 注册清单入口的 IDE 可见性

## 问题与修复

用户截图中，RustRover 将 `database::migrations::entries()` 标为私有，但生成文件已声明 `pub(crate)`。现场复现该红线；原生成入口在 Rust 编译器下可以跨同级模块调用，问题出在 IDE 对 `include!` 生成函数的可见性解析。

迁移和 Seeder 的 `pub(crate) const fn entries()` 固定声明在各自正式 `mod.rs` 中；`build.rs` 继续生成原模块声明，清单数据分别放入 `migration_entries.rs` 与 `seeder_entries.rs`，由入口按表达式包含。模块层级、导入路径、const 调用、清单内容与排序均沿用现有机制。没有扩大为 `pub`，没有关闭检查或编辑 target 中的生成文件。

参考 [Cargo 构建脚本代码生成](https://doc.rust-lang.org/cargo/reference/build-script-examples.html#code-generation)和 [`include!` 表达式／条目规则](https://doc.rust-lang.org/std/macro.include.html)。沿用现有 OUT_DIR 机制，未引入新依赖或额外注册流程。

## 验收

隔离源码与日志：`target/tmp/database-registry-ide-20261123/`；构建缓存复用 `target/tmp/model-fields-sync-20261123/cargo-target`。所有 Cargo 命令使用 Rust 1.94.0，经 `tests/build_guard.py` 设置项目内 TMPDIR 和独占目标目录。

- `build-main.log`：真实主应用 `cargo build --bin main` 成功。
- `test-final.log`：2项消费者测试通过，从同级模块在 const 和运行时上下文调用两个入口，核对迁移文件和注册内容。
- `discovery-add.log`、`discovery-remove.log`：只在隔离源码添加、删除探测迁移，两轮各2项通过；生成清单随文件变化更新，无需手工注册。两轮生成内容分别保留为 `discovery-add-generated.rs`、`discovery-remove-generated.rs`。
- `list-final.log`、`migration-help-final.log`：真实二进制执行 `artisan list --raw` 与 `artisan help migrate`，均以0退出。未执行数据库迁移或Seeder，不触碰现有数据库。
- RustRover现场确认启动文件由红色1项错误变为绿色检查；跳转直接定位到正式 `database/migrations/mod.rs` 中的函数声明。截图为 `rustrover-bootstrap.jpeg`。未关闭检查、清空IDE缓存或重启IDE。
- `accepted-inputs.json`：4份本批Rust源码／测试与验收快照一致；Rustfmt及差异检查通过。

探索性测试曾额外要求 const 引用与运行时引用的指针身份相同；新旧实现均未满足此断言，见 `test.log`、`baseline-pointer-check.log`。指针身份不是原接口承诺，最终测试核对可见性、const签名、清单内容与文件发现行为；未将探索性失败计为通过。原实现的编译成功也确认此问题不是缺少Rust访问权限。

生成文件的“不要编辑”黄色提示属于正常提示，不需要移除。以后修改生成逻辑应修改 `build.rs`；装配入口的可见性固定在正式源码。对应原则已补入项目规范的“代码与测试目录”。

此前验收结束时未暂存、提交或推送；用户随后授权单独本地提交本批修复，未授权推送。没有启动服务或临时Docker资源；隔离探测文件已直接删除，保留源码快照、验收记录及可复用缓存。结束可用空间约72.3 GiB。未扩展到Windows／Linux原生IDE或生产数据库验收。
