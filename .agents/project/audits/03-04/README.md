# 03-04 应用加密与密钥轮换

核对固定 Laravel 13.x 的[加密与轮换文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/encryption.md)、[Encrypter](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Encryption/Encrypter.php)、[Provider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Encryption/EncryptionServiceProvider.php)、[回归测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Encryption/EncrypterTest.php)和[key:generate](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/KeyGenerateCommand.php)。参考固定 [Goravel Crypt Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/crypt/service_provider.go) 的容器单例装配。核对旧版 `6d1a8192` 的 `encryption/{application,application_payload}.rs`、`console/command/{key_generate,key_file}.rs` 和固定 PHP 载荷回归。提取 AWS-LC 加密核心、顺序密钥环、PHP 字节字符串格式和只替换有效赋值的写入逻辑。

- 接通 `Crypt`、应用级 `encrypter` 和加密 trait。支持 AES-128/256-CBC 与 AES-128/256-GCM。CBC 验证 HMAC 后解密，GCM 验证完整认证标签。`APP_KEY` 支持原始及 `base64:` 密钥。`APP_PREVIOUS_KEYS` 用于回读，写入始终用当前密钥。缺失或非法密钥在解析加密服务时报告。
- `key:generate` 跟随引导选择的环境文件，支持 `--show`、`--force` 和生产确认。Rust 版使用进程锁、已生效配置匹配、暂存后原子替换及 Unix `0600` 权限。外部环境提供 `APP_KEY`、赋值重复或不安全的更新会报错并以非零退出。拒绝覆盖环境文件和错误退出码沿用旧版的显式失败行为。默认输出只显示成功状态。
- Rust 用类型化字节字符串和 `Result` 表达 PHP 的字符串及异常。序列化兼容 PHP 的字节字符串，PHP 数组和对象反序列化不在本单元。Cookie、Session、PHP 闭包签名和 Windows 实机验收仍有后续范围。

## 验收

macOS arm64、Rust 1.94.0。隔离副本 `/private/tmp/openrust-03-04.MeUqJ6/app-under-test` 以 03-03 候选为底，按本批范围合入共享文件。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。测试报告 380 项通过，含本批 3 项新增回归。真实 MySQL 与真实 S3 未在本批复跑。
- PHP 8.5.12 执行固定 Laravel `Encrypter`，四种 cipher 的原始与序列化载荷各双向验证，共 16 项。使用旧版固定 PHP 样例验证轮换，并拒绝篡改的 MAC、截断 GCM 标签、非法 IV、错误密钥和非字符串反序列化。真实 HTTP 覆盖当前密钥、旧密钥、密文失败、缺少密钥及多应用隔离。
- 实际 `./artisan key:generate`、`./artisan inspire` 退出码为 0。子进程验证 `--show`、`--env`、无交互取消、`--force`、外部密钥拒绝、缺失和重复赋值、行尾及权限。伪终端实际确认生产覆盖。8 组双进程并发均保留有效密钥和相邻赋值，8 次过期写入被拒绝。
- 三语 Rust 示例以根应用真实配置加载 `APP_KEY` 与 `APP_PREVIOUS_KEYS`，在 Kernel 请求作用域执行。`npm run build`、12 项常规浏览器组合和 6 项加密页正文、锚点及侧栏检查通过。工作区锁定依赖解析与 `git diff --check` 通过。

下一批为 03-05 密码哈希和重哈希，前置为 01-01 应用配置与装配。
