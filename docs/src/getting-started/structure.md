# 目录结构

根应用沿用 Laravel 职责目录，并用普通 Rust 模块声明连接源码。

| 路径 | 职责 |
| --- | --- |
| `app/` | 应用控制器、模型、Provider 等业务代码 |
| `bootstrap/app.rs` | 应用配置、Provider、模块和路由装配 |
| `bootstrap/providers.rs` | 显式应用 Provider 清单 |
| `config/` | 可编辑的 Rust 配置函数 |
| `routes/api.rs`、`routes/web.rs` | 应用 API、Web 路由声明 |
| `database/` | 应用迁移与种子 |
| `resources/`、`public/` | 应用资源与公开文件 |
| `storage/` | 应用运行文件 |
| `tests/` | 根应用与真实宿主装配测试 |
| `vendor/openrust/framework/tests/` | 框架公共契约与集成测试 |
| `modules/` | 可选并存的模块化业务组织 |
| `vendor/openrust/framework/src/lib.rs` | 独立框架库，公开 `openrust::` |
| `src/main.rs` | 根应用二进制 `app` 的薄入口 |

普通业务可直接写在 `app/`，无需先建模块。`modules/example/` 是唯一综合示例，示例自己的路由、迁移、语言和视图留在该模块；它不是另一份完整应用骨架。

`app/mod.rs`、`routes/mod.rs` 和 `modules/mod.rs` 是 Rust 显式编译入口，不是运行期自动扫描器。新增源码需登记相应 `mod`；新增 Provider 还需登记应用 Provider 或模块清单。

对应固定 Laravel 13.x 官方章节：[structure](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/structure.md)。本章仅说明已落地的当前入口及边界，尚未覆盖该官方章节全部能力。
