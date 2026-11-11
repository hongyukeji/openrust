# 01-02 第十五部分：容器环境解析回调

2026-11-11 完成本批验收。基础 Container.resolveEnvironmentUsing/currentEnvironmentIs 已接通；Application 默认环境检测另列 CT-ENV-APPLICATION，01-02 整体仍未完成。

## 固定参考及旧版核对

- Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc`：[Container](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 的 resolveEnvironmentUsing/currentEnvironmentIs、flush 及 resolveConcreteFromAttributes；回调接受 array|string，支持 bool|string 返回及异常传播。无回调时 false，setter 只替换回调。
- 同时核对 [LoadConfiguration](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Bootstrap/LoadConfiguration.php) 第 55–57 行的 detectEnvironment/resolveEnvironmentUsing，及 [Application.environment](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php)。Application 无参返回名称、传空数组返回 false，与基础容器和自定义回调不是同一规则。该默认检测/接线尚待下一批，不能将本批自定义空数组返回名称当作 Laravel 默认。
- 固定文档 container 绑定/解析及 configuration#determining-the-current-environment。三语正文仅增加已验收的显式容器回调，标明 Application 缺口。
- 编码前检查旧版 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 的 container.rs set_binding_environment/use_application_binding_environment 与 tests/unit/container/environment_binding.rs。旧版使用固定环境枚举，未提供此通用 callback 公开 API；提取环境变更不驱逐既有解析结果、应用隔离等回归依据，不把旧版枚举机制冒充当前契约。Bind/BindWhen 的声明次序与属性缓存规则留对应属性批次。
- Goravel 固定 v1.18.0 的编译装配保持不变。本批不改 main.rs、bootstrap/、Application、配置或依赖。

## 正式接口与行为

- resolveEnvironmentUsing 接受 Option<EnvironmentResolver>，Some(Arc 回调) 安装，None 清除。currentEnvironmentIs 精确转交借用的 EnvironmentQuery::Name/Names，保留参数形状、数组顺序、重复项、空字符串和星号，不由基础容器解释通配符或环境文件。未安装时返回 Ok(Boolean(false))。
- EnvironmentResult 保留 Boolean 或 String(Arc<str>)，不强制压成 bool。bool::from(result) 与 bool::from(&result) 对应 PHP 真值转换：false、空字符串和 "0" 为假，其余字符串为真。Result 对应 PHP 回调异常，错误继续进入现有容器/HTTP 异常链。
- 回调在锁外执行，允许重入及自替换；并发替换不改变已经取到原回调的调用。旧捕获在锁外释放。更改/清除不驱逐服务缓存、无额外解析事件；flush 保留解析器。各容器回调独立。
- Rust 用借用枚举与 Arc 返回字符串表达联合类型，Option/Arc 表达 nullable callable，Result 表达抛出异常；回调要求 Send + Sync。名称查询不复制输入字符串；每次检查短读锁内克隆回调。不增加普通解析路径的调用，不宣称性能基准或零开销。

## 实际验收

macOS ARM64、Rust 1.94.0。工作区 287 项常规测试、隔离副本 268 项，另有 16 项实际命令子进程通过。fmt、严格 Clippy、locked/offline 独立 standalone release 通过。[Rust 结果](rust-results.json)。隔离构建关闭增量缓存以控制空间。

- 新增 9 项单元：无解析器、形状/顺序/重复/借用身份、两类结果和 PHP 字符串真值、缓存/事件不变、flush 保留、异常和 panic 后恢复、回调重入/自替换、锁外捕获析构、多容器隔离及真实线程并发替换/保活。
- 新增实际 HTTP 集成，6 次 TCP 请求验证显式 Provider 安装的自定义策略、bool/string 返回、错误 500 脱敏、修复后重试及两应用配置/回调隔离。该策略刻意对空数组返回名称，不声称是 Application 默认匹配器。
- 新增 environment:probe 真实 Artisan，安装并执行显式回调。原发布 [58 HTTP 与 6 Artisan](http-results.json)、[40 命令进程](command-results.json)通过，包含预期失败。
- [54 个三语 Rust 示例](example-results.json)编译执行通过；三语构建和根路径、/docs/ 的三语桌面/移动共 12 项浏览器检查通过。[文档结果](docs-results.json)。

## 最小范围与复现

[10 文件 tree](changed-tree.txt)：3 个框架实现/契约、4 个测试文件和 3 个三语页面。其他 Application/gRPC、清单、配置和工作区 WIP 排除。[范围说明](scope-notes.json) · [SHA-256](candidate-sha256.json)。

```sh
python3 .agents/project/audits/01-02o/prepare_candidate.py /tmp/openrust-01-02o-review
cd /tmp/openrust-01-02o-review
cargo +1.94.0 fmt --all --check
CARGO_INCREMENTAL=0 cargo +1.94.0 test --workspace --locked --offline
CARGO_INCREMENTAL=0 cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_INCREMENTAL=0 cargo +1.94.0 build --locked --offline --release --features standalone
```

三个 verify 脚本接收副本路径；prepare_candidate 从 git-commit.txt 的不可变提交恢复并核对，不读取工作区 WIP。

## 剩余边界和下一批

- CT-ENV-APPLICATION：按固定 LoadConfiguration 在配置加载后接入默认环境检测和解析回调；保留 Application 无参/空数组/多模式差别、通配符及 CLI --env 优先级。检查并提取旧版相关实现，用实际 HTTP/Artisan 验收，在环境属性之前完成。
- 全局实例与 ArrayAccess 的 Rust 对应、SelfBuilding、类/参数属性、新实例及自动接口投影仍依依赖顺序推进。事件 when 与完整流式/终止作用域依原计划。
- 01-02 未整体完成；Windows 实机未验收，未推送。可选 JSON 业务封装仍按 04-02 提取旧版 ApiResponse/PageMeta。

本地代码提交 `4e1cd9b9af5a717b4a8bffad1544795d1d0b4997`，未推送。10 文件从不可变提交恢复校验通过。清理独占 target 1,283,852 KiB；磁盘可用约 51.77 GB，最近额度剩余 93%。保留候选源码、必要发布产物及共享缓存。
