# M3 Mailer 与 Policy 生成器 Spec

> 状态：完成；适用门禁和双库生成项目验收已通过，提交记录见 `progress.md`。
> 依赖：目录收尾已完成；复用 `openrust::mail::Email` 与 `openrust::auth::Policy` 既有契约。

## 范围

1. 增加 `openrust generate mailer <snake_case_name>`，生成独立邮件类型，实现 `Email` 并注册到 `framework/src/mailers/mod.rs` 与 `src/main.rs`。
2. 增加 `openrust generate policy <singular_resource>`，生成资源级 Policy 包装，提供 read/create/update/delete 的精确权限常量和 `authorize_*` 方法。
3. Policy 生成要求项目 manifest 已启用 `auth`；不自动修改依赖或迁移。
4. 两个生成器复用当前标识符规范和模块注册规则；非法名称、重复模块、缺少前置依赖必须报错，不覆盖已有文件。
5. 为两个 generator kind 添加 CLI、文件输出、注册、拒绝路径和不覆盖测试。
6. 在 CLI README 记录命令、输出和 Policy 所需 feature；更新 CLI core 文档、活跃待办、计划进度与验证记录。
7. 生成 PostgreSQL/MySQL 项目并实际执行 `cargo check` 与 `cargo test`，确认两类输出可编译且测试可运行。

## 排除

不修改邮件传输、策略存储、授权规则或公共框架 API；不生成数据库迁移、路由或业务权限种子数据。
