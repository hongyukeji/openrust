# 默认 API 组的限流配置

2026-11-22，补齐 `Middleware::throttleApi`，不在默认应用中启用限流或注册示例策略。

## 来源与适配

对照固定 Laravel Framework v13.40.0（`1449bf241017087fbd38c118ecaabef3f6797fdc`）的 [Middleware 配置](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Configuration/Middleware.php) 和 [HTTP Kernel 优先级](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Http/Kernel.php)，通过官方原始源码在线核实。查阅同一固定版本的 MiddlewareTest，未发现 throttleApi 专用用例。旧版 `v0.x` 的 `http/rate_limit.rs` 提供路由限流层，但没有当前配置接口，继续复用当前主线已有的限流中间件、策略注册与 Redis 后端，不引入额外依赖。

- Rust 没有默认函数参数，显式使用 `throttleApi(Some("api"), false)` 表达上游默认策略及初始缓存后端；传入其他名称选择对应策略，`None` 关闭默认 API 组限流。保留空字符串及 `"0"` 不启用限流的上游行为。
- 最后一次策略赋值生效。第二个参数为 true 时复用 `throttleWithRedis()`，后续 false 不撤销已启用的 Redis 别名。
- 默认 API 组在装配时按有状态会话、限流、路由绑定的顺序构造。自定义 API 组覆盖默认组，不受配置调用顺序影响。
- 补齐默认优先级中的两个限流中间件类型，使路由认证先于限流、限流先于会话认证及路由绑定，保持 Kernel 的固定上游顺序。
- 策略仍由应用 Provider 的 boot 注册；不存在自动注册的 `api` 策略。默认应用入口、路由及 Provider 均未修改。

## 验收

工作流位于 `target/tmp/api-throttle-20261122/`，从 `revision.txt` 所记 Git 基线建立独立 `source/`，仅覆盖本次框架实现、独立测试及三语中间件指南。Rust 1.94.0，TMPDIR 和独占 CARGO_TARGET_DIR 均位于该工作流。最初同盘迁移并复用已闲置的基础控制器验收缓存；随后触及32 GB磁盘停线，安全停止本次编译，待其他 Cargo 退出、核对占用并取得各目录排他锁后，清理项目编译与文档产物，保留锁 inode、源码、日志、依赖及数据。实测释放 61.40 GB（57.18 GiB），可用空间从31.41 GB恢复至92.80 GB，详见 `disk-cleanup.json`。恢复后沿用正常增量和并发。

- 全 workspace／all targets 严格 Clippy 已通过，日志 `clippy.log`。初次检查发现新测试缺少 Application 契约导入，已补齐并重新通过。
- 独立 Redis 8.10.3、Sanctum 开启下运行 `cargo +1.94.0 test -p openrust-framework --features sanctum --test api_throttling --test throttling --test named_limiters --locked -- --skip cluster --skip redis_fixture_process`，16 项实际用例通过。包含新增8项、既有命名策略1项和既有限流7项；两个筛除项为本次未运行的集群专项和子进程启动入口，不计入通过数。
- 新增真实 TCP HTTP 覆盖默认关闭、429 与重试响应头、独立分桶、排除与 web 范围、路由绑定前消耗、策略覆盖与关闭、自定义组覆盖、缺少命名策略时失败、Redis 跨应用共享配额与切换保留、认证先于限流、两种调用顺序下的有状态会话先于限流。既有用例额外验证 Redis 跨进程共享与原子竞争。日志 `http-tests.log`、`redis-result.json`，临时容器已回收，见 `redis-cleanup.txt`。
- `cargo +1.94.0 test -p openrust-framework --no-default-features --test api_throttling --locked -- --skip redis` 通过6项实际 HTTP 回归；该次仅筛除 Redis 专项，不计入通过数。`cargo +1.94.0 test -p openrust --test http --locked` 通过16项，覆盖命名组、优先级、API 前缀、路由清单及真实 HTTP。日志 `http-feature-off.log`、`application-http.log`。最终全仓 `cargo +1.94.0 fmt --all --check` 和 `git diff --check` 通过，日志 `fmt.log`。本次两个源码文件与三语指南均已逐字节核对与验收快照一致。
- 三语文档内容检查及 VitePress 构建通过；既有浏览器检查12种组合、全部导航检查6组、新增配置示例及注册策略链接检查6组通过，覆盖 `/` 与 `/docs/`。日志 `docs-build.log`、`docs-rebuild.log`、`docs-browser.log`、`docs-scope.log`、`docs-api-throttle-browser.log`。定向浏览器脚本曾在 URL 更新后立即检查标题而发生等待竞态，改为等待标题可见后六组通过，未改产品源码。

本次不修改缓存或数据库驱动内部行为，未重跑三库、持久缓存驱动或 Redis Cluster 专项；本记录不将历史驱动验收计为本轮执行。不涉及真实第三方供应商或部署。本次按用户授权提交至本地，不推送。
