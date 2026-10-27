# 仓库宿主的业务模块

`apps/mod.rs` 是仓库根 `server` 的业务模块注册表；现有 `apps/home.rs` 展示了一个已接入的路由模块。这里采用 Rust 编译期模块和显式插件组合，不扫描目录、不在运行时加载代码。

仓库根 `apps/*.rs` 模式让框架源码留在根 `src/`，业务实现留在 `apps/`；更新框架源码时，开发者不需要把业务实现搬入或维护在框架 `src/`。这个模式与根 package 共用 `Cargo.toml`、feature 和锁文件，新增依赖需修改根 manifest，框架升级时也需处理 manifest 冲突。需要应用依赖与升级周期独立时，应在框架仓库之外维护独立应用工作区。

添加业务模块时：

1. 在 `apps/` 下创建 Rust 模块，例如 `apps/billing.rs` 或 `apps/billing/mod.rs`。
2. 在 `apps/mod.rs` 用 `mod billing;` 声明它。
3. 在 `Server::build` 中通过现有 `AppBuilder` 注册该模块的 `Plugin`、服务或路由。路由模块可像 `home` 一样通过 `.routes(billing::register_routes)` 接入。
4. 业务依赖仍写入根 `Cargo.toml`；运行参数放根 `config/` 或 `.env`，不通过配置文件动态加载 Rust 模块。

例如，模块声明和组合关系是显式 Rust 代码：

```rust,ignore
mod billing; // apps/billing.rs

fn build(app: &mut AppBuilder) {
    app.plugin(billing::BillingPlugin)
        .routes(billing::register_routes);
}
```

`src/main.rs` 保持唯一启动入口，只调用 `openrust::run`。模块注册集中在 `apps/mod.rs`，新增业务能力不需要增加 binary target 或启动命令。

`openrust new` 默认在 `apps/<name>/` 生成独立 Cargo package，具有自己的 `Cargo.toml`、`src/` 和根 `config/`，不会自动编译进根 `server`；但生成器会把它登记为当前源码 workspace 成员，因此它仍与根 workspace 共用依赖版本和 `Cargo.lock`。当前生成器不支持把项目创建到源码仓库之外。只有显式声明的 Rust 模块属于根宿主。若要独立锁定依赖和升级应用，需自行维护框架仓库之外的应用 workspace；根 `apps/*.rs` 则用于直接在源码仓库内开发、集成和示例组合。
