# ADR-010：应用提供数据库 Seed 函数

- 状态：Accepted
- 日期：2026-10-25

## 决策

应用通过 `SeedPlugin::new(seed_fn)` 注册普通异步函数；框架提供 `db seed` 运行期命令并传入已初始化的 `Db` 句柄。回调返回框架 `Result<()>`，错误直接交给命令调用方。

不定义 Seeder、Factory trait 或框架级数据模型。应用自行决定种子数据、调用方式和幂等性。SeedPlugin 需要与 DatabasePlugin 一起注册；命令不接受额外参数。

## 理由

第一代计划要求 `src/seed.rs` 使用普通 async 函数初始化应用数据，并由 `db seed` 调用。Rust 闭包和 `Future` 已能表达扩展点，无需额外 trait 层。
