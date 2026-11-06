# 注册与模块组织

Laravel Modules 是默认集成的第三方扩展，用于把同一应用内的一组业务代码、路由、配置和资源组织在 `modules/<名称>/`。模块使用与根应用相同的 Container、Provider 和路由机制。模块机制位于独立 `openrust-modules` 包，应用依赖该扩展和框架；框架不反向依赖它。

默认应用与新生成应用以 `modules/mod.rs` 为集中编译和注册入口，应用在 `ApplicationDefinition::configure_providers` 中通过 `ModuleProviderBuilderExt::module_manifest` 安装它返回的清单。将新模块声明为 Rust 模块，再通过公开 `ModuleDescriptor` 和 `ModuleManifest` 登记 Provider 及其他贡献。可以从唯一 `modules/example/` 学习实际接线，无需复制完整应用骨架。

默认 `registered` 模式以已编译清单决定模块和启用状态，不要求根 `modules_statuses.json` 或运行期 JSON 扫描。Rust 不能直接执行目录中新出现的源码；增加模块或修改编译清单后需重新编译。

`registered` 模式不提供持久启停写入。尝试执行状态变更会在写文件和派发变更事件前拒绝；不要用它模拟已经保存状态。需要可持久启停时，显式选择[文件状态模式](file-state.md)。

Rust 模块显式编译是语言约束；默认 `registered` 是本项目获准的应用约定。独立扩展 `openrust_modules::ModulesConfig::default()` 仍保留上游 `file` 默认，不因本应用约定改写。当前入口不代表动态加载或完整 laravel-modules 命令族已实现。

根应用清单的核心接线如下，模块 Provider 仍通过统一管线注册；删去 `module` 和 `enable` 两项即不装载该模块，单独的 Rust `mod` 不启用它。

```rust,ignore
openrust_modules::ModuleManifest::new()
    .with_application_root(root)
    .module(example::registered_descriptor(root, config))
    .enable("example")
```

`registered_descriptor` 是本应用的模块入口，不是框架内建函数。唯一示例同时通过普通 Provider 复用 `ModuleFile` 初始化回调；注册路径和文件发现路径各执行一次，不创建第二容器。
