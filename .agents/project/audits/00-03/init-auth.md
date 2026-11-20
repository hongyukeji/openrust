# 初始化完整认证扩展接线

2026-11-20，骨架、实现及本批 macOS ARM64／Windows ARM64／Linux x86_64 验收通过。

## 契约与来源

接续 [初始化历史验收](init.md)、Passport-09、API-F01/F02、API-S01、API-P01、API-O01/O02 已验收安装前置。固定 Laravel Framework 13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc`、文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8`、Fortify 1.43.0 `3db382a3d5d6a0d537de32a190ce3b945e621713`。init 是项目扩展入口，单项包的原命令契约继续保留。

检查本地 v0.x 的 init/install 文件清单，未发现等价统一安装命令；复用当前已验收的路由安装、资源发布、迁移和包安装职责，不引入旧安装状态数据库。

## 实现范围

- 默认向导推荐 all；显式选择 api/broadcasting/reverb/grpc/passport/fortify/passkeys/socialite/precognition/oidc/sso，按实际 Cargo 特性装配；应用骨架仍无默认启用的可选包。
- 可选包向应用容器注册共享资源安装服务。SourceInstallation 持有原 fail-fast 源码锁，统一安装和单项安装复用包资源阶段；不递归启动 fortify:install、passport:install 或 install:api。
- 当前程序未编入所选资源服务时，集中进行一次 bootstrap build，内部 init:resources 进程取得同一源码锁发布全部包资源，之后统一最终构建、原迁移入口一次执行。已编入安装器时直接调用服务，只需最终构建。交接锁期间另一源码操作若先取得锁，本次明确失败并保留内容，不无限等待。这是 Rust 可选特性须编译后才能执行安装服务的适配，不是每个包重复构建。
- `/user` 保持 auth:sanctum；Passport 配置独立 api 守卫；已有不兼容守卫报告冲突。OAuth 消费方客户端由应用显式注册。
- 新 Fortify 配置为无界面模式并启用邮件验证，Passkeys 按选择启用；已有配置保留，force 显式覆盖安装清单。保留 app/actions/mod.rs 中无关模块。force 不传给 Passport 密钥生成，不覆盖历史迁移。
- 新 Passkeys 配置写入前检查精确 HTTPS 域名 Origin，开发允许 HTTP localhost；拒绝 IP、路径、凭据、query/fragment。缺少 APP_URL 时按 APP_PORT 补 localhost；已有值需要开发者明确修正。第三方凭据、回调、账户绑定、应用路由和前端重置地址单列前置，不宣称生产 SSO 或全球退出已配置。
- 补齐完全缺少 APP_KEY 赋值的环境文件；仅对待生成键写入原 key:generate 要求的单行、不带引号空赋值，随后调用原密钥命令。已有有效密钥不变。首次 Passport 初始化准备缺失的密钥目录；半套密钥仍拒绝。

## 验收位置与已知失败

全部 Cargo 位于 `/private/tmp/openrust-init-auth-20261120/source` 的 f82018bbd archive 加本批源码输入，独占同批 target；Rust 1.94.0，增量关闭、3 jobs、dev/test debug=0。真实数据库统一由 mysql_acceptance.py 创建隔离 MySQL 8.4.13。所有失败源码与日志保留，后续追加正式结果。

早期编译的 trait 导入/可见性/遗留死函数已修正。真实初始流程定位并修正缺失 APP_KEY、空赋值引号、缺失 storage 目录和 WebAuthn Origin 预检；后续失败来自夹具长前缀触发 MySQL 索引名上限、测试路由插入到闭包、删除后恢复公钥丢失权限，分别修正夹具，未放宽生产校验。完整安装/迁移/原始 HTTP/重复与 force/单项互通/守卫冲突及缺失半套密钥已通过。额外修正重复 --env 的首值优先，和应用既有引导契约保持一致；最终版完整认证与 Chrome 回归通过。原生格式门禁曾发现一处模块排序，已修正并复验。

三语构建、根/子路径桌面/移动 12 组合及初始化内容 6 组合已通过，正式结果见下文。本批不推送；原有 plan.md 与其他协作者内容不纳入本批暂存。

## macOS ARM64 最终验收

- `native-resume.log`：init 14、Passport lifecycle 10、Fortify provider 3、根配置 1，共 28 个函数通过；六 feature（pail,passport,fortify,socialite,oidc,reverb）全 workspace/all-targets `clippy -- -D warnings` 通过。
- `native-rest.log`：Rust 1.94.0 全仓 fmt、同六 feature 根构建、真实根 HTTP／Pail／Artisan、关闭可选功能检查通过。
- `base-profile.log`：真实 PTY 与密码隐藏、Redis、SMTP 只连接不发信、嵌入 Reverb WebSocket 101、选定环境、迁移失败重试、实际 HTTP／令牌与自定义前缀通过。
- `auth-final.log`：无可选 feature 的主程序启动默认 all，集中 bootstrap/final build，全部迁移；未插入测试路由前 `/up`、`/` 200、`/api/user` 401。重复/force/without-migration 保留 APP_KEY、Passport RSA pair 和所有历史迁移；单项安装互通、守卫冲突和缺失半套密钥拒绝通过。
- 同一生成应用真实 MySQL 8.4.13 上 Sanctum 200／Passport 401、Passport 200／Sanctum 401。完整 Fortify HTTP／CSRF／Session 轮换、三封 SMTP、邮件验证／资料／密码／2FA／恢复码／重置／限流／正常退出，以及严格生成应用 Clippy、功能关闭构建通过。
- Chrome 加固定官方 JS 的 Passkeys 注册／确认／可发现登录／重放拒绝／Session 与用户隔离／过期／删除／限流／独立挑战行锁计数器并发／级联删除通过。官方客户端 `@laravel/passkeys` 0.4.1 `c4ee7572993aadb40c630dedc349fe74cca0c89f`，服务端参考 0.3.0 `2e7f893429d35acff45ac16d05ddcf883372a20e`，使用原 lock 的离线安装及 Vite 8.3.6 bundle。
- `docs-build-final.log`、`docs-final-check_browser.mjs.log`、`docs-final-check_init.mjs.log`：三语构建，根/子路径、桌面/移动 12 组合和初始化内容 6 组合全部通过。既有大 chunk 提示保留。误在无 dist 的 live 目录启动浏览器脚本的失败不计正式结果。

22 份最终输入 SHA 见 [清单](init-auth-inputs.json)，live 与独立快照逐字节一致；其中一份原样恢复的 Fortify console/mod.rs 只作为快照输入留证，不形成提交差异。Cargo.lock 无改动，无新依赖。所有 Cargo 均在独立快照运行。

macOS 独占 target、文档 dist 和客户端 node_modules/bundle 回收释放 8,764,808 KiB，清理后 Available 73,731,748 KiB；源码、失败夹具、日志、共享缓存和全部数据卷保留，证据 `cleanup-mac.json`。Windows/Linux 使用各自新批次目录，后续另记。

## Windows ARM64 实机验收

使用原有 Parallels Windows 11（10.0.26200.0），本轮开始为 stopped；独立目录 `C:\Windows\Temp\openrust-init-auth-20261120`，原生 `aarch64-pc-windows-msvc` Rust 1.94.0 `4a4ef493e`／LLVM 21.1.8，2 jobs、增量关闭、debug=0，共享工具链和 Cargo 缓存保留。18 份相关输入 SHA 与最终清单一致。MySQL 仍由宿主统一 wrapper 建立 8.4.13 tmpfs 实例，通过只接受该来宾私网 IP 的临时 relay 访问；不是 Windows Docker 验收。

首次 PowerShell `artisan.bat init --no-interaction` 从无可选 feature 的主程序出发，实际完成 bootstrap／内部资源发布／最终重建及全部迁移，证明在 Windows 可执行文件锁下工作。随后夹具误检查不存在的独立 config/passkeys.rs（正式配置位于 config/fortify.rs），应用已经成功；修正夹具后用原样保留的源码和密钥续验，并只更新临时数据库连接。`windows-evidence/initialization-first-fixture-failure.log` 保留首次成功及夹具失败，`windows-evidence/initialization.log` 和 `windows-e2e-resume.log` 为后续正式结果。

PowerShell 普通重复与 CMD force 重复均通过，APP_KEY、RSA pair SHA、迁移数量保持；生成应用 `/up` 与 `/` 200、未认证 `/api/user` 401。随后同批原生 init 14 函数全部通过，真实 ConPTY 向导隐藏密码、连接失败取消及配置不变通过。首次离线构建缺少 serde_cbor_2 的记录保留，按固定 Cargo.lock 补取依赖后通过；临时 relay 在连接关闭时记录的 ConnectionReset 不影响验收退出码 0。未将历史基础矩阵冒充本批 all 验收，未启动 Windows Docker Desktop，也未声称 Windows 浏览器 Passkeys 已验收。


## Linux x86_64 原生验收

Ubuntu x86_64 独立 `/tmp/openrust-init-auth-linux-20261120`，Rust 1.94.0 bookworm 容器编译原生 ELF，应用在原生宿主执行；2 jobs、增量关闭、debug=0。本批复制专属 registry/git 缓存，不修改宿主工具链或共享缓存；18 份相关输入 SHA 与最终清单一致。数据库由同一正式 mysql_acceptance.py 入口在该宿主建立 MySQL 8.4.13 tmpfs 实例。

`acceptance.log` 首轮构建成功，包装器使用 sudo 后未显式传递 target 值，夹具查找错误路径，在安装前失败。修正包装器并复用实际 target 后，`acceptance-resume.log` 的默认 all、两阶段重建、迁移、原始 HTTP、重复/force/单项互通及冲突拒绝均通过，后续夹具需要 cargo fmt 时发现基础 Rust 镜像缺少 rustfmt/clippy。本批专属派生镜像补齐官方 1.94.0 组件，重新准备无 feature 主程序，完整重跑 `acceptance-resume2.log` 最终通过。

最终覆盖默认 all 的真实 bootstrap／内部发布／最终重建／迁移、原始 HTTP、APP_KEY／Passport RSA pair／历史迁移保留、单项安装互通、冲突和半套密钥拒绝、独立 Sanctum／Passport 令牌守卫；完整 Fortify 注册／CSRF／Session 轮换／三封 SMTP／邮件验证／资料／密码／2FA／恢复码／重置／限流／正常关停，迁移回滚与重建、生成应用严格 Clippy 及功能关闭构建均通过。Linux 无浏览器用例；真实 Passkeys 浏览器生命周期依据本批 macOS Chrome 实测，不称 Linux／Windows 浏览器已测。

## 交付边界

统一安装接线与本批生成应用整合闭合；已有单项 API 簇的协议、供应商及定制能力沿用各自审计，不在本批重复实现。Rust 可选 feature 的服务必须先编译再执行，因此未编入安装器时使用集中 bootstrap 加统一最终构建；此适配已有三平台实测。

真实外部供应商测试应用凭据、获准回调、业务账户绑定、前端重置地址、身份提供方与生产 SSO／退出策略仍由应用准备；本批不会创建生产客户端、代填凭据或宣称生产全局退出可用。Windows Docker Desktop、托管 AWS 和其他数据库／发布目标仍按原发布矩阵跟踪，不以本批 MySQL 8.4 受控验收替代。


## 最终资源与证据

[验收元数据与日志 SHA](init-auth-validation.json)保存全部正式结果、已知夹具失败、工具链、平台输入核对及清理数据。原始日志与失败源码保留在 `/private/tmp/openrust-init-auth-20261120`；Linux 原始源码／日志继续保留在该宿主专属批次目录。Windows 已回收本批 target 与 source/target，来宾实际释放 7,975,813,120 字节，保留源码、日志与共享工具链缓存，恢复原 stopped 状态。Linux 回收本批 target／专属依赖缓存副本及临时组件镜像，释放 6,253,152 KiB，剩余 102,601,048 KiB；保留所有源码、失败夹具、日志、共享构建缓存与基础镜像。两边所有数据卷核对保留，未删除数据库卷。

最后宿主 Available 77,990,272 KiB（79.862 GB／74.377 GiB），高于 48,828,125 KiB 停线；额外传输缓存副本已回收。无本批活跃编译或测试进程，不推送；只核范围本地提交，协作者 plan.md 及未知 WIP 均保留。
