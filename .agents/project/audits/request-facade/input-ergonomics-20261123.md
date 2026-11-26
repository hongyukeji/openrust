# Request 输入读取简化（2026-11-23）

## 依据与边界

用户在模型字段侧对话确认采用简洁的 Request 读取方式，并授权实现；本轮没有 Git 操作授权。机制继续按固定 Framework v13.40.0 的 [InteractsWithInput](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Http/Concerns/InteractsWithInput.php) 和 [InteractsWithData](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Support/Traits/InteractsWithData.php) 核对。参考固定 [Goravel 请求契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/request.go) 的具名读取与可选默认值，不采用其不同的输入来源优先级。

已检查同级 `openrust-v0.x/vendor/openrust-framework/src/http/request/input.rs` 的 `input(&str)`、query 与 post；复用当前主线已有的解析、缓存及标量转换实现，不复制旧请求体架构。Rust 不支持默认参数和普通方法重载，因此提供普通、`_or` 和 `_strict` 入口；请求体继续异步读取，解析错误通过 Result 传播，不阻塞线程或使用隐藏的默认值吞错。

## 正式接口

| 入口 | 契约 |
| --- | --- |
| `input("user.name").await?` | 正文／查询合并、点号和通配符规则不变；返回 `Option<Value>` |
| `query("name")?` | 仅查询集合精确顶层键 |
| `post("name").await?` | 仅正文精确顶层键，原有 JSON／表单选择不变 |
| `input(None)`、`query(None)`、`post(None)` | 原有整组读取保留；`Some(&str)` 也仍可用 |
| `input_or(key, default)` | 缺失时默认；显式 null、空串、零与 false 保留 |
| `query_or(key, default)` | 精确顶层键缺失或 null 时默认，按固定基线 retrieveItem 的默认值契约；既有 query 的 Option 读取不变 |
| `post_or(key, default)` | 沿用 post 的按键规则，缺失或 null 时默认，其他假值保留 |
| `integer(key)`、`float(key)`、`boolean(key)`、`string(key)` | 缺失时分别使用 0、0.0、false、空字符串；string 返回 Stringable |
| 类型方法的 `_or(key, default)` | 普通转换并自定义缺失默认值 |
| 类型方法的 `_strict(key, default)` | 严格转换，缺失或类型不合格使用默认值，不作为返回 422 的表单验证 |
| `str`、`str_or`、`str_strict` | 对应 string 方法的同义入口 |

Request 实例和门面分别提供以上入口，门面继续借用同一请求，不复制正文或引入第二份全局状态。不添加含义重叠的 `get` 别名。JSON／输入优先级、GET/HEAD 表单正文不解析、JSON 源优先、输入大小限制和缓存错误行为保持不变。

原有类型方法的三参数 Rust 调用改为 `_or`（strict=false）或 `_strict`（strict=true）；运行时 strict 值通过明确分支选择。同步已有 Fortify、Passkeys 和测试消费者，以及欢迎页 `post_or("redirect", "/")` 的实际使用；欢迎页仍拒绝非字符串重定向。通用 key 参数接受 `&str` 或 `Option<&str>`，动态 String 使用 `key.as_str()`，不依赖泛型参数中的隐式 Option 解引用强制转换。英文、简体与繁体 Request 文档同步契约及调用示例。

## 验收记录

临时材料全部位于 `target/tmp/request-input-ergonomics/`。修改前文件备份仅用于保留协作者差异，位于其 `before/`，不纳入交付。使用项目构建守卫、Rust 1.94.0 及项目内可复用 Cargo 目录；不操作数据库或数据卷。

- 首轮 `tests.log`：原有 request_facade 11 项、request_facade_full 17 项、request_input 30 项通过，后者包含真实 TCP HTTP、JSON／表单／multipart、严格及普通转换、响应与错误路径。
- `tests-followup.log`：新增 5 项边界回归和 request_query 3 项通过。新增回归覆盖直接键、嵌套／通配符、动态键、缺失与 null、假值、严格及普通转换、重复读取、GET／HEAD 与 JSON 输入源、无效 JSON、非法查询和超大正文。动态键断言在最终测试中再次确认。
- 新增测试最初对 GET/HEAD 表单正文和 JSON 源优先级的两项预期不符，按现有实现和已固定语义修正测试，未为通过测试改变生产解析逻辑。原始失败记录保留。
- workspace 首轮检查发现泛型 key 不再隐式转换 `Some(&String)`，已将相关消费者改为显式 `as_str()`；同次检查另暴露 Fortify 密码视图宏与 getter 的在途不一致，单独跟踪，不以该次结果宣称全量通过。
- 运行中触及32 GB停线，欢迎页构建被守卫停止，不计通过。全项目守卫随后持锁回收编译产物，日志、源码、依赖下载及数据保留；具体释放以 `target/tmp/build-guard-events.jsonl` 为准，不算成本任务独立回收量。

## 最终复验与交付边界

macOS ARM64、Rust 1.94.0，最终相关源码／文档 18 份指纹见 `target/tmp/request-input-ergonomics/frozen-source.json`，复核与工作区一致。以下 Cargo 命令均通过 `python3 -B tests/build_guard.py --workflow request-input-ergonomics --target-dir target/tmp/rust-naming/cargo-target --` 启动，复用项目内 Cargo 缓存并由 Cargo 锁串行保护；正文和模板没有依赖外部临时目录。

| 命令与证据 | 结果 |
| --- | --- |
| `cargo test -p openrust-framework --test request_input_ergonomics --test request_facade_full --test request_input --test request_query --test request_facade`；`tests-frozen.log` | 66 项全部通过，包括最终默认值契约、动态字段名及实际 HTTP 消费者 |
| `cargo test -p openrust --test welcome`；`welcome-final.log` | 36 项全部通过，包括正文重定向优先、类型拒绝、语言保存与清除、CSRF 和查询参数处理 |
| `cargo clippy -p openrust-framework --lib -- -D warnings`；`clippy.log` | 通过 |
| `cargo check --workspace --all-targets --exclude openrust-fortify`；`workspace-without-fortify.log` | 通过，包含根应用、其余扩展和测试目标 |
| `cargo check --workspace --all-targets`；`workspace-check-followup.log` | 未通过；本轮动态键适配问题已消除，仅剩 Fortify 密码视图的 4 个在途错误，不宣称完整 workspace 通过 |
| `npm --prefix docs run build`，通过 `--workflow request-input-docs` 守卫运行；`docs-frozen.log` | 三语内容、导航／链接检查和 VitePress 构建通过；存在原有 bundle 体积提示 |
| Rustfmt 检查本轮全部 15 份修改／新增 Rust 文件；`fmt.log`，以及限定范围的 `git diff --check` | 通过 |

Fortify 当前阻塞位置为 `vendor/openrust-fortify/src/http/responses/password_reset.rs:76`，宏实例仍调用 `request_password_reset_link_view()`／`reset_password_view()`，而当前对应名称是需要 view 参数的 setter。该文件已有命名迁移在途改动，本轮没有覆盖或调整它；对应两个宏各产生参数缺失及对 unit 调用 `ok_or_else` 的错误。不能据此判定 Fortify 已完成整体运行验收。

最终源码定稿后已完整重跑上述 66 项回归；中间 `tests-final.log` 编译期间调整过 query_or 的 null 默认值，形成一次旧编译组合与新断言不匹配，该组合不计通过。以冻结后 `tests-frozen.log` 为准，不以最早测试结果代替最终验收。

测试进程均已退出，本轮不启动常驻服务或 Docker 资源，不改数据库或数据卷。日志、源码备份与项目内可复用增量缓存保留；收尾时可用空间约64 GiB。未实跑 Windows／Linux、未重复执行数据库发布矩阵；本轮不涉及驱动、持久化格式或部署配置变更。没有暂存、提交或推送，既有 Git 暂存和其他协作者改动保留。

用户随后于同日授权单独本地提交本批 Request 输入简化改动。提交从本轮修改前备份提取最小差异，排除既有命名迁移和许可证暂存改动，不推送；上方“没有暂存、提交或推送”描述的是验收完成时状态。
