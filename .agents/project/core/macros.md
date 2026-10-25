# openrust-macros

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 过程宏 crate；通过门面 crate 对应用暴露属性宏。

## 当前实现

- `#[openrust::resource]` 标注 inherent `impl`，读取其中的 `list` / `get` / `create` / `update` / `delete` 无 receiver、非泛型关联函数，并生成 `openrust::http::Resource` 实现。
- 未识别的关联方法保留为普通辅助方法；属性参数、trait impl、动作 receiver 和动作方法泛型会产生编译错误。
- 门面 `openrust` re-export 宏；`apps/server` 集成测试通过 `ResourceMacroApp` 验证动作路由映射。

## 后续边界

- `Job` / `Event` derive 宏归 M4；此 crate 不负责生成器或运行期行为。

## 验证

`cargo test -p openrust-server` 覆盖宏展开后的五种动作映射；workspace gate 覆盖宏 crate 的 Clippy 与文档构建。
