# 03-07 Blade 全源注释与原样块预处理

状态：实现与本批实际验收通过，按下列 15 个文件核定本地提交，不推送。

## 固定依据与复用

- Laravel Framework 13.40.0，`1449bf241017087fbd38c118ecaabef3f6797fdc`：[BladeCompiler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/BladeCompiler.php) 的 compileString／storeVerbatimBlocks／restoreRawContent，以及 [CompilesComments](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesComments.php)。文档沿用固定 13.x 的 Blade displaying-data／comments。
- Goravel 1.18 的入口与运行结构本批不改；不借用其模板语法替代 Laravel。
- `../openrust-v0.x` 的 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf`，`vendor/openrust/framework/src/view.rs` 使用 MiniJinja；未发现能直接提取的 Blade 全源预处理器。保留现有已移植求值器和回归，仅增加本批必要词法接线。

## 实现与语言适配

先屏蔽源码中原有的完整 verbatim 块，再一次性删除完整 Blade 注释；之后才解析组件、指令与回显。删除注释可以连接指令名称、参数和表达式中的标记，不能将注释当作空白，也不能让删除后才拼成的 verbatim 获得保护。注释内的原样标记、原样块内的注释和转义顺序均按真实 PHP 对照。

原样块开头的 ASCII 空白保留在占位符外，继续参与既有 PHP 关闭标签换行消费。原样内容在文本或表达式的词法边界识别后恢复；字符串回显仍按正常转义规则处理，内部占位符不进入响应。没有完整分隔符的未闭合标记保留为文本。移除局部 comment_spans／raw_ends 推断，编译缓存版本 31 升至 32，旧产物重建。

Rust 使用应用内编译 AST、局部原样块列表和 Cow；不增加 PHP 产品运行时、全局可变状态或请求期锁。普通无标记源码／表达式可借用输入，变化只发生在编译阶段，不改变已缓存视图的执行路径。仍不支持任意 PHP 语句、未注册函数和所有 PHP 编译阶段；不得将本批称为整个 PHP／Blade 完成。

## 验收

PHP 8.4.14，Composer 2.8.12，Illuminate 全部精确锁定 13.40.0；复用既有测试 Composer 锁。198 组语料中成功 184、错误 14，较上一批增加 55 组。两次参考生成逐字节一致；夹具 SHA-256：`6ccd7854cf804aec5d8b5934279e7c2f5df445699e46fce95c7c4fd22f16a169`。覆盖注释位于回显字符串、运算式、指令名／参数、组件属性，原样块保护／转义／开头空白／多个块，以及字符串表达式中的原样恢复。

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

最终源码定向回归 39 项通过，含 198 组直接／真实 HTTP 对照和 962 组算术／HTTP 回归，错误 JSON／HTML 与恢复、缓存 31 失效及相同 Factory 多模板状态隔离。三语站点构建、12 项通用及 6 项源码预处理章节浏览器检查通过；首轮文档 inline Blade 注释被 Vue 解释，改用 code v-pre 后重建通过，未忽略错误。

根实际二进制验证新预处理／转义、算术、HEAD、/up、JSON／HTML 500 和恢复、view:cache／clear、inspire、原始 ./artisan inspire、SIGINT 退出 0 与端口释放，全部通过。全工作区全目标（features pail）严格 Clippy 退出 0，格式及差异检查通过。8 份源码／测试／参考与隔离副本 SHA 一致，32 份启动／配置／路由／Cargo 基线不变。不涉及数据库行为，未重复无关数据库验收；Windows 仍属发布前矩阵。

## 最小提交范围

```text
vendor/openrust/framework/
├── src/view/compilers/{blade_compiler,template}.rs
├── src/view/compilers/template/preparation.rs
└── tests/
    ├── {blade_arithmetic,blade_statements}.rs
    ├── fixtures/blade_statements.json
    └── reference/blade_statements.php
tests/blade_arithmetic_http.py
docs/{en,zh_CN,zh_TW}/the-basics/blade.md
.agents/project/
├── audits/03-07/blade-preparation.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

15 个文件；main、bootstrap、config、routes、Cargo 清单及锁不变。原有未跟踪补丁、core/ 和 translation-database/ 保留。本批只做本地范围提交，不推送。

## 收尾、授权与证据

本批远程闲置专属源码副本／target／registry／git／Composer 依赖清理前合计 5,322,536 KiB（约 5.45 GB），已直接删除；此前不存在且 SHA 匹配的 Composer 镜像已删除，共享 Rust 镜像与全部数据卷保留，数据卷清单前后一致。初次删除因容器生成文件属 root 而失败，随后仅对同一核定目录以 sudo 完成，未改动共享缓存或数据库。没有使用 Docker prune 或删除数据卷。

本地专属浏览器依赖及站点产物共回收逻辑字节 77,117,414，df 实际可用增量 80,900,096 字节，两者分开记录；清理后项目 Available 80,680,420 KiB（82.62 GB／76.94 GiB），高于 50 GB 停线。保留共享 docs/node_modules、源码、原有 WIP 与 `/private/tmp/openrust-blade-preprocess-20261119` 的实际验收日志、Composer 锁、SHA、清理与提交映射。

沿用最新真人取消人为额度停线的授权，正式额度工具当前不可用，不冒充实时读数，不启用 credits。原完整 Goal 仍 active。核心账本既有 66/66 不变；JSON 非有限数跨 ViewData、其他 PHP 表达式契约、Pail 异步身份解析／Windows 终端矩阵仍独立跟踪。按最新第一代范围继续核对 Passport／OAuth2 等 API 扩展及发布前验收，用户验收和正式发布授权前不改版本、打 tag、推送或发布。
