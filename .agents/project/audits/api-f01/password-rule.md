# API-F01 默认密码策略与确认规则依赖

2026-11-20。Fortify 安装模板前置：原生验证器实现 `rules::Password::{default,new,min,max,required,sometimes,mixedCase,letters,numbers,symbols}`、`Rule::confirmed()`、`confirmed`／`confirmed:字段`。不将本批称为完整 Password fluent API；defaults 回调、附加 rules、uncompromised 服务、条件组合及 current_password 不在本批验收范围，安装仍继续。

## 来源与适配

- 固定 Laravel Framework 13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 Password.php、ValidatesAttributes.php 和英文 validation.php。本地 v0.x 的 framework/src/validation.rs 与 validation/tests.rs 没有 confirmed／Password fluent 实现，未复制旧表单或旧认证协议。
- Password 保持隐式规则时序：缺失且非 required 跳过；nullable 使用 PHP blank 判定；内部 string／min／max 按普通规则执行；强度检查在其后按 Unicode Ll／Lu／L／N／Z／S／P 执行，混合大小写正则的 `.` 不跨换行。默认长度为 8；min 下限 1，max=0 不限制。数字、bool、null 的嵌套长度沿 PHP 标量转换；数组按元素数，但嵌套 Password 消息类别仍是 string。
- confirmed 按上游严格值比较，保留数值／字符串类型、嵌套数组键顺序及下标；JSON 数组／数字键对象在 PHP 中均属于 array。跟随验证器路径机制处理嵌套、转义和通配符依赖。普通规则与 required 的空字符串判定补齐 PHP trim 的 ASCII 空白与 NUL，避免 Rust Unicode trim 将上游有效字符错误跳过。
- 规则使用当前验证器的翻译、消息覆盖、属性名称、Bail 和错误包；不以字符串最小长度检查替代 Password 对非字符串的拒绝。Fortify 模板所需组合为 required／string／Password::default／confirmed。

## 验收证据

独立 archive 基线 `40f8b759b7130cf7a89df4a18f0da5d0644eb6a6`，快照、固定 PHP 完整依赖运行时、日志保留在 `/private/tmp/openrust-password-rules-20261120/`。本批不叠入后续协作者的改动。

- 完整 Illuminate Validation 13.40.0、其全部 Illuminate 依赖均固定 13.40.0；两份核心源码逐字节匹配固定 Framework。34 个依赖的版本／提交／文件树 SHA、autoload 引导文件 SHA 和 Composer lock SHA 记录在 fixtures/password-rule-manifest.json，完整 lock 单独保存。PHP 8.5.12 镜像固定 digest，执行无网络；未替代内层 Factory、Validator、翻译或规则。
- 771 组原 PHP 完整验证器对照覆盖缺失／nullable／required／sometimes、scalar 类型、Unicode 长度、强度、换行、确认字段、键顺序及嵌套通配符。Rust 同时核对通过／拒绝与每一条错误消息。输出 SHA256 `3d75b92687b5ec6515021201de621dac5d6e92537b12690211e1ebb20b4765df`；重复生成一致，源码篡改负样本被文件树校验拒绝。
- 三个新增函数：完整固定 PHP 对照、Fortify 默认组合／默认消息、真实 TCP／FormRequest。初轮对照揭示 required 在 NUL 字符串上的 trim 差异，修正后通过；失败与修正日志均保留。
- 验证器共30个不同函数与Fortify49个不同函数通过；补充自定义消息／属性名称及通配符参数后，全部验证器30函数、Rust1.94 workspace／all-targets features pail,passport,fortify严格Clippy／fmt、根main构建、真实HTTP／Pail／恢复／超时清理／SIGINT／端口释放及独立Artisan再次通过（regressions.log、final.log）。9份输入SHA与快照一致；专属target闲置核对后删除，Available释放6,284,072 KiB，剩余92,453,488 KiB；源码／日志和全部Docker卷保留。见source-sha256-password-rule.json／cleanup-password-rule.json。未执行 SQLite、其他数据库、外部密码泄漏查询或完整发布矩阵；三语功能正文保持全簇验收后填充。
