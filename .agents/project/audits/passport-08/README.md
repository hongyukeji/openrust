# Passport-08 API 安装与生成应用

2026-11-20：骨架、实现及本机验收完成，继续 Passport-09 定制与共存。V06 保持部分验收；Windows 嵌套重建分支未在本批实机验证，平台发布验收仍待。

## 固定参考与复用

框架13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 ApiInstallCommand、Passport13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 的 InstallCommand；SHA见记录。框架标签源码再次按提交SHA下载，逐字节相同。只读检查同级旧版 `6d1a81924` 的 console／command、CLI模板和Passport目录：没有对应安装器，未复制占位；复用当前source_edit锁／原子源码替换、vendor发布、Cargo构建识别和真实包命令。

隔离基线 `141ed4aef090784103bd679ff63490cc2684f8ce`，9份源码／测试／正文输入与根基线逐字节核对。未叠入同期Fortify提交和WIP，也未更改Cargo依赖／锁文件。没有新增PHP运行时oracle。

## 行为与原生映射

`install:api --passport`启用随项目提供的Cargo feature、保留其他defaults，接默认User的HasApiTokens／users provider、api Passport guard及auth:api路由，遵守既有API配置和多级API_PREFIX。发布／源码接线后真实重建并运行passport:install，最后重建已发布配置／迁移；各阶段失败传播。自定义缺少锚点／既有冲突guard保留并明确报未完成，不冒称成功。已有路由保留，API --force仅重写路由，不旋转密钥。

正式公开包InstallCommand保留--force／--length=4096。完整已有密钥保留，残缺密钥明确失败；显式force交给真实keys命令。发布配置／五迁移并挂载配置加载器；发布与锁失败传播。真实终端迁移／个人客户端两项默认yes，非法确认重新询问，EOF停止确认；-n执行两项，重复成功可能新增个人客户端。管道且未-n不等待输入，明确pending。迁移重建后的子进程状态真实检查，SQL失败后可重试；生产确认不绕过，包force不转给migrate。

Composer选项不适用，Cargo feature／构建承担本机包安装装配。包命令额外--without-migration-prompt明确只发布并报告pending，外层转发；这是有意原生扩展，固定PHP外层Passport分支没有转发该选项。独立包命令配置加载器接线也是编译配置的原生适配。Windows使用两个重建目录避开当前运行二进制锁；本批只验收macOS，不将静态分支称为Windows实机通过。

## 实际验收

证据 `/private/tmp/openrust-passport-install-20261120/`，保留脚本、成功／失败日志、固定参考、SHA和源码覆盖包。

- 最低Rust1.94.0；统一wrapper实际MySQL8.4.13。新增根集成函数从默认feature真实CLI复制干净应用：首装／重装／API force、缺feature／源码锁／残缺密钥／guard冲突／构建失败／生产拒绝／真实SQL连接失败／成功重试、配置与五迁移／密钥保留、管道行为／真实PTY默认yes／拒绝个人客户端／非法回答重试及包force生成2048位密钥全部通过。
- 生成应用实际迁移／真实Eloquent User／PAT创建和资源Guard、机器client_credentials不能冒充用户、权威撤销后401，在默认及/api/admin/两个前缀通过。测试端点只写测试生成应用，正式根应用没有测试端点；服务成功SIGINT退出并释放端口，失败也由RAII停止子进程。
- 10个Passport目标113个普通函数通过（3个浏览器函数保持ignored，本批未重跑）；configured guard2、专用Eloquent MySQL Session1、Session扩展5、Sanctum安装回归1、新安装函数1，共123个不同函数。既有授权协议未改，本批不把既往Chromium结果算作新增实跑。
- strict workspace／all-targets features pail,passport Clippy、fmt通过。三语23个h2构建，根及/docs/挂载12项通用＋6项Passport浏览器检查通过。根features二进制HTTP／OAuth错误／no-store、原始./artisan、帮助与SIGINT端口释放通过。最终仅Windows路径选择及测试显式target参数修正后，完整生成应用验收和strictClippy再过；根smoke／既有协议回归在此前原生等价源码运行。
- 失败记录保留：初始未声明reqwest改用现有Factory；长测试前缀触发既有队列外键名称长度限制，缩短前缀后完整迁移通过；错误输出流断言和测试路由注入匹配到内层Ok导致启动失败，修正测试后完整复验。未执行非MySQL数据库、Windows实机和新PHPoracle。

## 收尾

仅核定本批本地提交，不推送。保留同期Fortify与其他协作者改动。源码包／日志保留后按归属直接清理闲置专属source／target／生成应用，df与全部卷前后清单见cleanup.json；共享缓存和全部数据卷保留。下一单元Passport-09。
