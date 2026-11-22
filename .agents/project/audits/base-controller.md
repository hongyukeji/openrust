# 应用基础控制器

2026-11-22：固定 Laravel 应用骨架 `06d016a364a37430eec9cbc52209adce3d7667ce` 的 [Controller.php](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/app/Http/Controllers/Controller.php) 是空抽象类，已通过官方原始源码在线核实。旧版 `v0.x` 的应用控制器目录没有对应基础控制器，不复用无关业务实现。

按用户确认的 Rust 适配方案，将 `app/http/controllers/controller.rs` 的空结构体改为空 `Controller` trait，`WelcomeController` 通过 `impl Controller for WelcomeController {}` 声明公共接口关系。Rust 不支持类继承，trait 用于后续共享默认方法；当前为空，不提供运行时行为，也不限制其他控制器必须实现它。不新增渲染、授权、验证方法或路由约束，欢迎页数据处理仍由具体控制器负责。

保留文档注释“应用基础控制器。”及定向允许 dead_code 的原因“应用控制器的公共扩展入口”。空 trait 的实现不会让它自动成为被调用的接口，因此此处仍保留该诊断说明。

此前空结构体验收已通过 Rust 1.94.0 的全 workspace 格式检查、全 workspace／all targets 严格 Clippy 和结构体导入探针，相关证据保存在 `target/tmp/base-controller-20261122/`。这些结果只代表修改前的结构体版本。

本次 trait 版本在同一工作流的 `source-trait/` 独立快照验收，基线见 `trait-revision.txt`，仅叠加两个控制器文件；TMPDIR 和独占 CARGO_TARGET_DIR 位于该工作流内，复用原工作流构建缓存。执行 `cargo +1.94.0 fmt --all --check` 已通过，证据为 `trait-fmt.log`。`cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings` 已通过，覆盖实际 WelcomeController 的 trait 实现，证据为 `trait-clippy.log`。`git diff --check` 通过。本次不涉及数据库、缓存或文档站，未新增重复实现的测试。本次按用户授权提交至本地，不推送。
