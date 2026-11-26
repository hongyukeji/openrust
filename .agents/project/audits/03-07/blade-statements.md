# Blade 指令边界与引擎空白

## 固定对照与复用

基线 `30d06b37132fd7fe62b0c37862997374d1553185`。对应固定 Laravel 文档 [Blade Templates](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md) 的指令、原样内容及片段章节。固定 Framework 13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [BladeCompiler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/BladeCompiler.php)、[PhpEngine](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Engines/PhpEngine.php) 和 [View](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/View.php)。

旧版 `../openrust-v0.x` `6d1a81924` 的 vendor/openrust-framework/src/view.rs 使用 MiniJinja，未发现可直接复用的 Blade 词法器。复用现主线 Parser、Factory、HTTP、上一批 PHP 装配与算术回归，不引入旧 MiniJinja 或改变 main、bootstrap、Runner。固定 Goravel 的编译运行职责保持不变。

## 实现与语言适配

- 语句起点采用 PHP 非 Unicode PCRE 的 ASCII 单词字节规则；内置名称匹配编译方法大小写，自定义名称按注册键区分大小写。读取完整名称及可选 `::member`，不把未知命名空间名误认成内置名前缀。
- 按实际消除的完整注释源区间判断相邻单词边界，正文再次出现 `{{--` 时仍采用最初开标记对应的消除区间；连续注释按源顺序回溯。verbatim 保留字面内容，其边界与转义 @ 分开处理。裸 `@@` 不再被任意折叠。
- 记录当前指令是否有同行参数，名称到左括号只允许空格／制表符。复现生成 PHP 操作后紧邻 LF、CRLF、CR 的消除；不误吞空格前缀后的换行，不双重消除空参数结束指令后的两行。echo 保留自身后继换行。
- 内置与自定义冲突时先采用已注册处理器；custom_stop 在标准结束标签前处理已注册条件，保持应用／工厂隔离。Rust 自定义回调仍返回既有类型化输出，不把 PHP 源码作为可执行字符串引入运行时。
- 在每个 Blade 引擎求值边界以 PHP ltrim 的字节集合清理开头空白：空格、制表符、LF、CR、垂直制表符、NUL。换页符与 NBSP 保留，普通 HTML 文件引擎不改变字节。采用原字符串原位移除前缀，不声称未经测量的性能收益。
- 编译版本 30 → 31，自动使旧产物失效。既有夹具用显式空字符串 echo 分隔原本紧邻 ASCII 单词／指令的结构，包括 Rust concat! 跨块拼接；保持原展示字节，避免用非 Laravel 语法验收。

## 片段空白的实际结论

固定 PHP 8.4.14、真实 Illuminate View Factory／CompilerEngine 生成的结果：

| 调用 | 输出 | Composer 次数 |
| --- | --- | ---: |
| fragments(right, missing, left, left) | `R1:1L2:2 R2:2 L3:3  L4:4 ` | 4 |
| fragments()，Rust allFragments() | ` L1:1 R1:1` | 1 |
| fragments([]) | 空字符串 | 0 |
| fragment(missing) | `L1:1 R1:1` | 1 |
| fragment(left) | ` L1:1 ` | 1 |

命名片段缓冲区保留自身空白；缺失名称回退到完整视图，适用引擎 ltrim。原 /fragment-composed 旧预期的额外空格不符合固定 PHP，按参考结果调整；没有对所有响应或所有片段套用 trim。全部片段及重复名称、空列表的次数均另外断言，原有真实 HTTP 回归同步通过。

PHP 的 allFragments 是 protected，公开无参 fragments() 才是对应入口；Rust 使用既有显式 allFragments() 表达无名称参数的分支，三语正文已说明。

## 参考夹具与验收

PHP 参考脚本在真实 Illuminate 13.40.0 上运行，仅为匿名组件命名空间提供最小容器对象；没有声称运行完整 PHP HTTP 应用。错误／弃用分流沿用固定 HandleExceptions 的策略。Composer 清单专供测试，额外精确固定 ramsey/uuid 4.7.6，以执行真实默认 @once；PHP 不进入产品依赖。初次参考执行缺少此依赖，以及误从外部调用 protected allFragments 的问题，均修正后再生成，没有将基础设施失败冻结为预期。

最终参考含 **143 组语料（140 组原有 + 3 组嵌套开标记注释）、2 组覆盖优先级、5 组片段场景**。143 组中成功 130、错误 13；两次 PHP 生成逐字节一致。夹具 SHA-256：`4dbfae1f7954f0c000d62484ba978e35d06e298b34d01217ea7b254aae858c80`。

Ubuntu amd64，Rust 1.94.0 Debug，复用暂停前现有隔离源码和缓存，覆盖核定文件后先验证 SHA：

```sh
cargo test --locked -p openrust -p openrust-framework \
  --test blade_statements --test blade_arithmetic --test view_finder \
  --test console --test feature --test artisan_entry
cargo build --locked --bin main
python3 tests/blade_arithmetic_http.py
cargo clippy --locked --workspace --all-targets --features pail -- -D warnings
cargo +1.94.0 fmt --all -- --check
npm --prefix docs run build
```

- 最终 39 项测试函数通过：语句 6、算术 5、既有视图 5、根 Console 7、Feature 14、Artisan 入口 2。143 组语料直接求值与真实 HTTP 逐项对照，另有 JSON／HTML 错误及恢复；962 组算术／HTTP回归保留。覆盖工厂隔离、覆盖优先级、custom_stop、旧缓存失效、文件引擎与 Blade 区别及片段次数。
- 根实际二进制验证 ASCII／中文边界、内置大小写、转义、命名空间、注释源区间、关闭标签换行、PHP 开头空白集合、错误 JSON／HTML 500 和恢复、HEAD、/up、view:cache／clear、inspire、原始 ./artisan inspire、SIGINT 退出 0 和端口重新绑定。
- 全工作区全目标严格 Clippy 最终退出 0；首轮一处 sliced_string_as_bytes 已按建议修正后完整复验，没有添加 lint 忽略。格式、差异及 Python AST 检查通过。
- 9 份源码／测试／参考与隔离副本 SHA 一致；Cargo.toml／Cargo.lock、main、bootstrap、config、routes 共 32 份基线文件不变。此前远程旧快照通过的结果没有充当最终新源码证据。
- 三语正文已同步指令边界、空白及片段；最终站点构建、根路径／docs 子路径的 12 项通用浏览器和 6 项章节／页面宽度检查全部通过。
- 本批不改数据库或 Worker，没有把历史 MySQL／Windows 结果算作本批验收。完整跨平台矩阵仍按发布前计划执行。

## 最小文件范围

```text
vendor/openrust-framework/
├── src/view/compilers/{blade_compiler,template}.rs
└── tests/
    ├── {blade_arithmetic,blade_statements,view_finder}.rs
    ├── fixtures/blade_statements.json
    └── reference/blade_statements.{php,composer.json}
tests/blade_arithmetic_http.py
docs/{en,zh_CN,zh_TW}/the-basics/blade.md
.agents/project/
├── audits/03-07/blade-statements.md
├── audits/07-01/blade-arithmetic.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

共 17 个候选文件，仅在验收及核定范围通过后提交本地，不推送。当时保留了历史工作区补丁、核心草稿和数据库翻译扩展资料。历史补丁现已完成[清理复核](../worktree-inventory.md#历史补丁清理)，其他资料的后续处置见[盘点更新](../worktree-inventory.md)。

## 授权与后续边界

本批曾于额度剩余 12% 暂停，补丁与缓存保留。之后真人先授权有限恢复，再解除项目的人为额度百分比停线与仅收尾限制，并将原 Goal 卡片设为 active，get_goal 已确认；未使用额度重置或启用 credits。正式额度工具当前不可用，不将旧的 12% 读数冒充实时额度。磁盘停线、按批验收本地提交、不推送与归属清理继续有效。

此批只闭合已支持语法的指令词法／引擎空白，不宣称任意 PHP 或整个 Blade 完成。全源编译预处理仍需独立核对，尤其指令名称与参数之间、表达式内部的 Blade 注释及其与原样块的处理顺序；当前解析器仍有相应未覆盖边界，应继续推进，不能以本批结果跳过。既有 JSON 非有限数视图数据边界、Pail 认证自动解析及发布前 Windows 矩阵继续跟踪。核心 66/66 仅保持原有范围。


## 收尾与证据

已直接回收远程专属源码／target／registry／git／Composer 依赖约 6.10 GB、本地站点和专属浏览器依赖约 77.10 MB，另删除此前不存在且摘要匹配的临时 Composer 镜像。数据卷前后一致，保留共享 Rust 镜像、文档依赖、数据库、源码和验收记录；未清理共享 Docker builder。本地清理前后 df 增量 80.68 MB，与文件逻辑字节和远程释放分开记录。提交前项目 Available 80,700,308 KiB，即 82.64 GB（76.96 GiB），高于磁盘停线。 原暂停前本地临时证据目录本轮未找到，因此没有复用其日志作为最终证明；本轮重新记录最终测试、PHP 比对、Composer 锁、SHA、浏览器、清理结果和提交映射于 `/private/tmp/openrust-blade-statements-20261119`。项目源码、夹具和审计文件均保留，远程旧验收目录已按归属清理。
