# 聚合应用事实

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：原根集成宿主迁入同一 workspace 内的 examples/demo，完整验收责任保留并重新通过本轮适用门禁。

根 openrust 是框架库，不含业务启动 bin。examples/demo 是根 Cargo workspace 的成员，共用根 Cargo.lock，package 为 demo，只有 server 默认 bin；薄 src/main.rs、bootstrap/app.rs、routes/web.rs 与 app/ 通过静态模块声明连接。运行时从应用工作目录加载 config/public/storage。

框架扩展继续用 openrust:: 公共 API；本簇不改 Application/AppBuilder/Plugin/认证/数据库/队列语义。examples/demo/tests/server 保留全部原应用/进程/真实服务测试，根 tests/core/http/queue 保留框架公开契约；根 default-members=["examples/demo"]，根无参数 cargo run 默认 server；CI 的 --workspace 门禁覆盖框架和 demo，双库另行实际执行。

根与 demo 目录无参数 cargo run 均实测 /health 200、公开 HTML 首页 200 与 SIGINT exit 0。薄入口仅在 workspace 根启动时切换到应用目录，配置与静态资源保持应用职责。原进程测试从根启动并验证 HTML 与 SIGTERM；双库各 11 targets/46 项及最终 CI workspace feature 组通过。旧布局验收只作历史证据。新布局实际结果见 [testing](../memory/testing.md)，当前待办见 [todo](../todo.md)。
