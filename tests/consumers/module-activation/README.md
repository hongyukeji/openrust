# 生成应用的模块持久启停 consumer

本目录只有安装器与公开测试 fixture，不增加 Cargo package、依赖或第二套启动器。先使用当前框架 CLI 建立普通 fresh 应用（不带 `--db`、`--auth`、`--grpc`），再运行：

```sh
python3 -B tests/consumers/module-activation/scripts/install_generated_fixture.py \
  /path/to/fresh-application /path/to/openrust
cargo test --manifest-path /path/to/fresh-application/Cargo.toml \
  --test module_activation \
  generated_application_persists_module_activation_without_hot_resource_changes
```

安装器核对生成应用的 workspace `openrust` 路径、当前模板结构、目录归属与符号链接，拒绝重复安装。它仅新增生成应用的 `tests/module_activation.rs`，不改 manifest 或生产 Provider；回执记录类型和文件 SHA。正式验收应确认实际执行一个测试，不能将过滤后的零测试当作通过。

fixture 编译生成应用真实的 `app/bootstrap/config/modules/routes` 入口，包装原 Application 并完整转发其 hooks，只追加命名编译绑定及独立运行 module.json。真实运行发现 ActivationFixture 后，其 Provider 贡献 `/api/module-fixture`；重复 Provider 声明由内核去重，重复文件路径由同一执行会话初始化一次，HTTP 经 Inject 检查计数。原持久状态、跨 App loaded 和路由断言保留。这不证明未编译代码加载、class aliases、deferred加载或完整模块管理矩阵。

测试从原应用 `config()` 取得清单，再用公开 `application_code_sources_with_overrides` 指定包含显式应用身份/URL的 Testing、独立 `application_root`、`state/modules.json` 与 memory 缓存。来源加载以私有临时目录和独立命名空间为根，没有该目录的 dotenv 文件，不读取生成应用或框架部署的 dotenv，不改它们的状态文件；不要求真实 DB/Redis。memory store 按既有扁平选项结构配置，不使用额外 `options` 子表。

实际 `TestApp` 和 `App::run_command` 验证未知第二名称零写、`--all` 覆盖显式错误名称、大小写/Studly 参数写 canonical 状态键。禁用后当前 App 路由仍可用，下一 App 不加载模块而宿主 `/api` 保留；再启用不热挂第二 App，第三 App 才重新加载。断言同时检查公开 `ModuleRepository`/`Module` 的 loaded 与持久 enabled 状态、未知值保留。

所有返回错误的分支都会先尝试异步关闭全部已建立的 App，再清理由原子建目录和归属标记保护的自有临时目录。该测试使用进程内公开 Router；真实 TCP/跨进程重启及文件写并发仍由对应主控门禁单独验收。
