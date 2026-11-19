# API-F01 密码重置前置：默认 email 验证

2026-11-20。独立默认邮箱验证批次；密码重置正式 HTTP、安装与三语整体验收继续开发，不将此依赖称为 API-F01 完成。

## 固定来源与范围

- Laravel Framework 13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 `ValidatesAttributes::validateEmail`，Egulias EmailValidator 4.0.4 `d42c8731f0624ad6bdc8d3e5e9a4524f68801cfa` 的 lexer／RFCValidation／parser 和原测试。
- Doctrine Lexer 3.0.1 `31ad66abc0fc9e1a1f2d9bc6a42668d2fbbcd6dd`；Symfony polyfill-intl-idn／normalizer／mbstring 1.33.0，准确引用及 PHP 文件 SHA 见 [manifest](fixtures/email-rule-manifest.json)。保留 Egulias MIT 许可证。
- 查阅本地 v0.x `validation.rs`／`validation/runner.rs`。旧规则依赖 validator::ValidateEmail，其引号／注释和长度契约不等于固定 Laravel，因此未复制实现。
- `Rule::email()`、字符串 `email`／`email:rfc` 和 `rules::Email` 的 new／Default／Into<Rule> 接入原生验证生命周期，错误键 validation.email。其他参数明确 InvalidRule，未将 dns／spoof／strict／filter 或完整 fluent API 冒称已交付。
- 按上游 lexer／游标流程翻译默认 RFC 判断，先执行 Laravel 的非字符串及 CR/LF 拒绝。保留引号、注释、domain literal、特殊字符、域标签及域总长度处理；不额外将 local>64／总邮箱>254 的 warning 升级为失败。
- Unicode 标签采用已有 ICU4X UTS46 mapping／NFC、transitional 四字符转换与原始 Punycode 长度计算，只取标签过长，其他 IDNA 诊断保持非致命。新增显式 idna 1.1.0 依赖复用 Cargo.lock 既有版本，没有引入 DNS 查询。

## 对照与验收

独立 git archive 基线 `2383fdbda8df68151f142c738deca31f5ae20735`，源码和日志 `/private/tmp/openrust-email-validation-20261120/`。该基线已包含同期 Passport-09 的已提交结果；在途密码重置未叠入本批快照。

- 原 PHP `validateEmail` 方法实际执行；边界替代仅为 Collection／Container 以及 PHPUnit 基类。真实 Egulias／Doctrine／Symfony 类参与判断，未以自写正则产生期望。
- [PHP 夹具](fixtures/email_rule.php)提取原 RFCValidation 四组 data provider，组合短 token、Unicode／长度边界、类型与 CR/LF，追加固定种子 6000 组组合，共 9381 组 UTF-8／JSON 值。单独记录 1 组上游非 UTF-8 字节输入；Rust 字符串不能表示该值，未将其称为 Rust 对照覆盖。
- 固定 PHP 8.5.12 镜像输出 SHA256 `22a0bcfedc8823dd50c01f42a1ed7079bdec2e4a67ee0ffa9018aa9e6e538482`。源码 SHA 门禁、重复生成及篡改源负样本；见 prepare_email_rule.py。
- 三项新测试：全部 9381 组匹配；fluent／字符串入口与 wildcard／nullable 行为；实际 TCP／FormRequest 的 200／422 和 Unicode／引号／非法值。原 validation_http 20、validation_paths 2、validation_web 2 及 Fortify 43 个不同函数回归，MySQL 8.4.13。
- 首轮夹具入口与类型编译错误、Unicode leading-mark 长度差异、Clippy 单元结构 Default 诊断保留日志；修正后复验。

Rust 1.94.0 workspace／all-targets features pail,passport,fortify 严格 Clippy、全仓格式、根 main 构建和真实 HTTP／Pail／恢复／超时清理／SIGINT／端口释放／独立 Artisan 均通过（regressions.log）。9 份输入与快照 SHA 一致；专属 target 闲置归属核对后直接清理，数据卷前后一致，源码与日志保留；见 source-sha256-email-rule.json／cleanup-email-rule.json。未运行其他数据库／SMTP／DNS／完整发布矩阵；三语产品页随 Fortify 全簇验收填充。
