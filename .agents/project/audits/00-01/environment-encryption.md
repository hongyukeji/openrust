# 00-01 环境文件加解密

固定 Laravel 提供 `env:encrypt` 和 `env:decrypt`，支持四种 AES 规格、环境选择、路径和文件名覆盖、可读格式和增量重加密。OpenRust 沿用既有 Laravel 兼容加密器。整文件保留原始字节，可读模式保留变量名、原始值、出现顺序和重复项。更新前验证全部旧条目，相同值复用密文。旧版 v0.x 的 C023 只有待开发记录。 [固定 Laravel 配置文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/configuration.md#environment-file-encryption) · [固定增量测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Console/EnvironmentEncryptIncrementalCommandTest.php)

Rust 使用中性的进程变量 `ENV_ENCRYPTION_KEY`。终端输入不回显，预先提供的密钥不输出，只显示本次随机生成的密钥。输出被静默时须提供已知密钥。目标先写入私有临时文件，成功后原子替换，未授权覆盖使用排他落盘。`--prune` 在加密文件落盘后复验并删除源文件。`.gitignore` 允许跟踪加密文件，明文继续忽略。

以合入 Seeder 构建时发现的 `a5ea37585` 为隔离基线。三项真实 Artisan 进程测试覆盖整文件逐字节恢复、环境及输出路径、错误密钥、损坏载荷、增量重复项、无变化不重写、覆盖保护和失败时保留源文件。macOS 伪终端跑通隐藏密钥的输入。官方 PHP 8.5 CLI 运行固定 Laravel `Encrypter`，四种算法分别验证整文件和可读格式双向互操作。三语正文命令实测解密结果一致，macOS 明文权限为 0600。格式、严格 Clippy、`./artisan inspire`、VitePress 和 15 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本处理。

00-01 的配置缓存仍待交付，跨平台终端实机验收按发布矩阵。
