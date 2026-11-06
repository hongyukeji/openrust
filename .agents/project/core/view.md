# `openrust::view`

> 受众：AI Agent
> 更新：2026-10-31
> 摘要：可选服务端 HTML 模板能力。

## 当前实现

- Cargo feature `view` 启用 MiniJinja 文件 loader；默认构建不增加模板加载能力。
- `ViewServiceProvider` 从 `[view]` 读取 `directory`（默认应用工作目录的 `views/`）和 `auto_reload`（默认关闭），构建 `Views` 服务。
- `Views::render(name, context)` 返回 Axum `Html<String>`，仅接受 `.html` 模板。模板名拒绝绝对路径和遍历；文件 canonical path 必须位于模板根内，阻止符号链接越界。
- `.html` 自动 HTML 转义；strict undefined、每次渲染 100,000 fuel；模板读写/语法/执行错误以内部错误处理，错误来源不作为响应正文返回。
- `server.root_target = "view:index.html"` 会在 router 组装时检查插件与模板，任何缺失均启动配置失败；默认宿主仍使用 `file:/index.html`。
- `openrust new` 默认启用该 feature；根应用与 modules/example 的视图能力按根 feature 与配置启用；业务模块可提供命名空间模板及发布覆盖，根通用模板与静态资源仍按应用目录加载。

## 边界

模板同步运行，不提供异步模板函数、数据库访问、组件系统、用户上传模板或远程 loader。开发期 `auto_reload` 会在每次渲染时清空模板缓存并重读文件，适合本地使用，不应在生产开启。视图不负责授权或业务数据查询，应用必须先授权并显式构造 context。邮件模板继续使用独立 `mail-templates` feature。

公共契约与决策见 历史决策 069，用户说明见 [视图指南](../../../vendor/openrust/framework/src/view/README.md)。

## D10-C 请求错误共享（限定子范围已验收，已提交 `784045c`）

`Views::render_for_request(name, &RequestInfo, &context)` 复用既有渲染器；map/struct 显式数据覆盖请求私有 errors，包括 null，其他 context 返回 `view_context_invalid`。Session start 后、handler 前恢复共享对象；无 Session 每请求独立空对象是可选 feature 的 Rust 适配。根 `view:` 与模块页面走同一公开入口，普通 `render` 仍只使用显式 context。

`RequestInfo::put_view_error_bag` 修改同一请求对象。若该对象来自已存 Session 错误包，修改经候选预算检查后同步到 Session，后续仍按原有保存/flash 老化规则处理；整包替换解除旧对象关联，缺失 fallback 不回写，普通修改不延长 flash。custom 契约与 codec 在状态锁外调用，重入明确拒绝，预算失败不发布部分变更。`Session::read_errors` 仍返回独立读取快照，不是另一个可写共享逃生口。

模板默认/命名包方法保留自定义 trait 派发；根 any 按固定 ViewErrorBag 使用默认包 count。参数桥接支持必需参数、null、列表/关联字典值、标量多参数和 nullable 格式，详见[用户说明](../../../vendor/openrust/framework/src/view/README.md)。键列表沿单次1024字段预算；不模拟 PHP 任意对象/布尔/浮点弱类型及弃用警告，差异不隐藏。

v9当前输入已通过默认798/31 ignored、全特性1571/105 ignored、双严格Clippy、严格Rustdoc及适用features/deny/MSRV；当前router106、HTTP7、view-only6通过，核心10/模块4/CLI1按限定输入等价证明复用。fresh普通生成7项、MySQL8.4.13模板认证2项与独立公开生成consumer1项、最终no-session/MySQL真实TCP均通过，隔离库/端口/敏感文件已清理；限定子范围已验收，已提交 `784045c`。证据及失败保留见[测试记录](../memory/testing.md#c04-d10-c-请求局部视图错误共享验收中未提交)，不关闭完整视图/HTTP/Session能力。
