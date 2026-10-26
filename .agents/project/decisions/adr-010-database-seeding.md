# ADR-010：应用提供数据库 Seed 函数

- 状态：Accepted
- 日期：2026-10-25

## 决策

应用通过 `SeedPlugin::new(seed_fn)` 注册普通异步函数；框架提供 `db seed` 运行期命令并传入已初始化的 `Db` 句柄。回调返回框架 `Result<()>`，错误直接交给命令调用方。

不定义 Seeder、隐式 Factory trait 或框架级数据模型。应用自行决定种子数据、调用方式和幂等性。SeedPlugin 需要与 DatabasePlugin 一起注册；命令不接受额外参数。

V1 在 `testing` 提供显式轻量 factory/fixture 工具，属于待交付补强，见 [补齐计划](../plans/v1-foundation-completion.md)。以普通 Rust builder 表达默认值、覆盖、批量与可复现数据，构造与持久化分开；复用实体、`TestDatabase` 和当前 seed 函数，不创建第二套 ORM、自动模型发现或隐式清库行为。公共签名及是否采用随机数据组件在实施 Spec 定稿；双库写入、隔离和错误边界须实际验证。

## 理由

第一代计划要求 `src/seed.rs` 使用普通 async 函数初始化应用数据，并由 `db seed` 调用。Rust 闭包和 `Future` 已能表达扩展点，无需额外 trait 层。
