# Provider 模板与默认骨架一致性

2026-11-21：调整 `provider.rs.stub` 的中文生命周期说明与标准 Rust 排版；保留模板原有的完整类型路径及 register/boot 共享显式生命周期。应用文件保持原样；采用 Rustfmt 默认排版，不使用格式化跳过属性。模板中的 `{{class}}` 由 `make:provider` 替换，应用已发布的自定义模板仍享有原有优先级。

参考固定 Laravel 框架 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [provider.stub](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/stubs/provider.stub) 及应用骨架 `06d016a364a37430eec9cbc52209adce3d7667ce` 的 [AppServiceProvider](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/app/Providers/AppServiceProvider.php)：保留空注册与启动钩子的职责及模板生成方式，异步签名和排版沿用现有 Rust 契约。已通过官方固定地址取得源码。

检查了 `vendor/openrust/` 下 54 份 Rust 生成／发布模板：

- Horizon、Telescope、Fortify Provider 补中文生命周期及方法说明；翻译残留英文授权注释，可执行代码未变。
- 广播、gRPC 路由模板的文件说明改为中文块注释。
- 默认模型、工厂、Seeder 与通用模板的示例逻辑差异属于应用定制，不机械覆盖；检查范围内未发现剩余纯英文说明注释。
- 在既有真实 Artisan 生成测试中核对输出，继续覆盖并发重试及已发布自定义模板。不要求默认应用与通用模板逐字一致。

验收快照位于 `target/tmp/provider-template-sync-20261121/source`，基线为 `e1cb3d8182be234064a43f265e2b8ce6836e6c19`，叠加受影响文件；使用独占 target 和项目内 TMPDIR，子进程工具链为 Rust 1.94.0。初次完整启动测试发现临时 Cargo 项目位于仓库内时被外层 workspace 识别，已为该独立测试夹具补 `[workspace]`；未改变应用运行行为。

## 验收与边界

- 修正后的隔离快照执行 `cargo +1.94.0 fmt --all --check`：通过。
- `cargo +1.94.0 test -p openrust --locked --offline --test application_skeleton --test startup`：2 项骨架、28 项启动测试全部通过。
- 扩展 Provider 模板另做 Rustfmt 检查，并核对移除注释／空白后的代码与原版一致。
- 将通用模板分别生成 ReportServiceProvider、Application、ServiceProvider、HttpFuture、ProviderResult，在两个异步钩子内使用 app，链接真实 openrust 库进行 rustc 编译：五种名称全部通过，仅有空类型未构造警告。
- Fortify 四处模型注册调用以当前 User 进行独立编译，缺少 Clone、TwoFactorAuthenticatable 和 PasskeyAuthenticatable；按安装器逻辑补齐后全部通过。密码重置调用未产生独立错误。本检查使用隔离缓存中的真实库，不替代完整安装与 HTTP 验收。
- 提交前核对上述模板、测试和默认应用文件，与通过验收的隔离快照内容一致。

本轮未重复全工作区 Clippy；前一版快照曾通过严格 Clippy，但不作为最终版本的完整验收结论。本次不涉及 SQL、缓存驱动或面板行为，未执行数据库／缓存矩阵及已安装扩展面板验收。

## 设计取舍

固定 Laravel 的通用模板与默认应用 Provider 职责一致，但注释文字不同，不要求逐字相同。Rust 官方 [生命周期省略规则](https://doc.rust-lang.org/reference/lifetime-elision.html)说明方法输出省略生命周期时关联 self；Rust 1.94.0 独立编译复现确认，返回的 async move 使用 app 时，省略生命周期版本失败，原有共享显式生命周期版本通过。因此模板保留原有签名和完整类型路径，避免导入名称与生成类名冲突；不添加模板与默认应用逐字相等的测试。默认应用文件保持原样，应用开发者在空 boot 内异步使用 app 时同样需要显式共享生命周期。

尝试读取固定 Goravel 应用 Provider 路径返回 404，未将该路径作为已验证依据。保留其他协作者改动；本次仅本地提交，不推送。
