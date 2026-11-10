# 00-02 默认 HTTP、静态文件与错误格式

2026-11-11 按本单元验收条件通过。Git 基线为 `197e7a0e`，在 00-01 候选上增量交付，未暂存、提交或推送。整个路由、文件系统及异常页面仍有后续单元，不据此标记整页完成。

## 对照与复用

- 固定 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 `src/Illuminate/Foundation/Http/HealthCheck.php`，对应内容协商及健康成功响应。
- 固定 Laravel skeleton `06d016a364a37430eec9cbc52209adce3d7667ce` 的 `routes/web.php`，首页保留命名视图意图。Rust 通过 `register()` 完成静态装配，通过 `Result` 传播失败。
- 固定文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8` 的 `routing#the-default-route-files`、`routing#view-routes`、`filesystem#the-public-disk`、`errors#rendering-exceptions-as-json`。
- Goravel 沿用 00-01 的编译型入口与应用装配核对，本批没有改变启动职责。
- 旧版 `6d1a8192:vendor/openrust/framework/src/http/health.rs` 的 `diagnose`，以及 `tests/server/bootstrap.rs` 的 `application_health_reports_failure_and_delegates_debug_rendering`。提取健康成功分支的 HTML/JSON 协商、应用名称转义和私有缓存头；诊断失败用例登记为事件依赖，未声称迁移完成。

## 已交付的范围

健康路由改用 `foundation::http::HealthCheck`，按请求返回 HTML 或 `{"status":"up"}`。HTML 使用应用配置名称并转义，GET 与 HEAD 保持一致的状态和内容类型。两应用测试证明配置不串用。

静态挂载保留网站 `public/` 在前、公共磁盘在后，磁盘完整挂载前缀只从 `url` 提取。根路由优先于静态文件，缺失文件继续进入正常 404 策略。配置、挂载和异常链路已有实现，本批补充隔离发布后的真实 TCP 验证及三语正文。

## 差异及后续依赖

- Laravel 健康端点会派发 `DiagnosingHealth`。当前事件分发器仍为骨架，诊断监听、异常报告和 down/500 分支归入 06-02。当前 `/up` 仅证明应用成功引导并能够处理 HTTP。
- 健康 HTML 复用旧版的独立页面写法，不使用 Blade、外部字体或 Tailwind。保留成功状态、标题和可读内容，不宣称视觉一致。
- Laravel 公共磁盘通常使用 `storage:link`。静态目录直接挂载是已确认的 OpenRust 扩展，不支持将 S3 当成本地目录。通用存储 API 和链接命令归入 03-01。
- 业务统一 JSON 响应按用户新指示登记到 04-02，优先提取旧版 `ApiResponse` 及测试。不会自动替换健康端点或 Laravel 默认异常结构。

## 文件范围

本批增量为 [13 个代码、测试和文档文件](changed-tree.txt)。必须带上未提交的启动配套时，共 [407 个候选路径](commit-tree.txt)。[文件清单](commit-files.txt)、[校验值](candidate-sha256.json)与[工作区排除补丁](scope-exclusions.patch)可重新构造核对，保留其他模块 WIP。

```sh
python3 .agents/project/audits/00-02/prepare_candidate.py /tmp/openrust-00-02-review
cd /tmp/openrust-00-02-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

## 实际验收

macOS ARM64，Rust 1.94.0。工作区 42 项测试通过；隔离副本 16 项测试通过。两者格式与严格 Clippy 通过，隔离 release 构建通过。另一副本重新构造后，407 个选定文件的 SHA-256 全部一致。

在根目录执行以下脚本，将隔离 release 二进制复制到新的临时发布目录，从不同工作目录启动。

```sh
python3 .agents/project/audits/00-02/verify_runtime.py /tmp/openrust-00-02-review
```

[实际结果](runtime-results.json)共 58 次真实 HTTP 请求和 6 次 Artisan 调用通过。覆盖首页 GET/HEAD、运行时视图更新、缺失视图 500、健康 HTML/JSON、普通及 API 的 HTML/JSON 404、HEAD 空正文、两级 `/storage/tmp/` 前缀、同 URL 网站优先、磁盘回退、Last-Modified 条件请求 304、目录和穿越拒绝、越界符号链接拒绝、根路由 405，以及 inspire/list/help。两个前缀分别启动独立进程，不依赖源码路径或当前工作目录。

三语站点 `npm run build` 通过，`tests/check_browser.mjs` 的根路径和 `/docs/`、三语桌面与移动共 12 项浏览器检查通过。未做 Windows 实机验收。

目录清单复核补充：另有 [1 个必要删除路径](deleted-files.txt)，即 `bootstrap/cache/.gitignore`。构造工具显式移除基线归档中的该文件及空目录；先前运行验收时文件仍在副本中，但不参与编译或启动。选定文件的历史 SHA-256 保留不变；删除规则已在累计 00-03 候选重新构造核对。已提交阶段的构造工具优先按 git-commit.txt 从不可变提交恢复，源工作区变化不影响复现。

## 本地提交

按用户 2026-11-11 新授权，已提交本地 Git：`54dff1f535aaab219e94d8452c6bf034865c3b6d`。本地提交范围为 13 个变更路径；前述“未提交”描述为验收完成时的历史状态。未推送。工作区中候选排除的其他模块内容保留。
