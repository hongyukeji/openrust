# Artisan 命令行

Console Kernel [引导应用](../architecture-concepts/lifecycle.md)后分派 Artisan 命令。应用骨架的命令列表和帮助无需 HTTP 监听或数据库连接即可运行。

```sh
./artisan list
./artisan list cache --format=json --short
./artisan help list
./artisan list --help
./artisan inspire
```

默认控制台路由注册了 `inspire`。执行 `./artisan inspire` 会随机输出一条引言及其作者，退出码为 0。

名言库与固定版 Laravel 一致，共 41 条。支持颜色的终端会将正文显示为粗体、署名显示为灰色。重定向时默认输出纯文本。可以用 `--ansi` 强制开启样式，或用 `--no-ansi` 关闭。

Windows 上使用 `artisan.bat`。编译后的应用通过 `main artisan` 接收相同的命令。

列表和帮助成功时返回退出码 0。未知命令和非法参数返回 1。标记为 `Planned` 的条目可以查看帮助，执行时返回 1。

通过 `withCommands` 接入 `app/console/commands/mod.rs` 中的命令类型。在 `bootstrap/app.rs` 的 `withRouting` 中声明 `routes/console.rs` 的控制台路由。应用命令可以替换同名占位项。

`routes/console.rs` 导入 `openrust::support::facades::Artisan`，通过 `Artisan::command("inspire", …)` 向 `register()` 执行期间绑定的控制台注册闭包，通过 `purpose()` 设置帮助和命令列表中的说明。闭包接收命令输出对象，正常返回时退出码为 0。

`Application::handleCommand` 调用 Console Kernel 的 `handle` 和 `terminate`。Kernel 完成引导后加载待实现命令清单，再注册应用命令并分派。


## 源码与发布入口

源码 `artisan` 脚本先进入应用目录，再将参数原样交给 Cargo，保留空格、空参数和非 ASCII 文本。脚本返回命令的退出码。

独立构建使用 `./main artisan list`、`./main artisan help inspire` 或 `./main artisan inspire`，无需 Cargo。配置相对于可执行文件解析，从其他工作目录启动也可运行。参阅[部署](../getting-started/deployment.md)。

编译入口不带模式参数时启动 HTTP，使用 `artisan` 时执行控制台命令。其他首参数会在创建应用前显示外层用法并返回退出码 2。Artisan 分派错误返回 1。外层分流是 Rust 单二进制的语言适配；Laravel 使用独立的 HTTP 和 `artisan` 入口文件。Console Kernel 的 handle/terminate 顺序仍由 `Application::handleCommand` 负责。

命令清单同时包含待实现条目。名称和帮助用于保留预定契约，不代表对应模块已经完成。通用自定义命令输入输出与本地化由后续 Artisan 单元交付。`artisan.bat` 尚未通过 Windows 实机验收。
