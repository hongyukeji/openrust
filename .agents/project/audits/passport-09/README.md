# Passport-09 定制与共存整合

2026-11-20：骨架、实现及本机验收完成。Passport-01–09计划范围本机验收闭合；Windows安装重建及发布整合仍待，V06不冒称跨平台发布完成。

## 固定参考与复用

Passport13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 的 Passport／Client／AuthCode／DeviceCode／Token／RefreshToken，固定SHA见 upstream-sha256.json。检查旧版 `6d1a81924`，其注册及安装路径不满足当前真实模型契约，未照搬占位；复用当前 Client／Token／RefreshToken 的类型注册、真实 Eloquent 模型生命周期及既有连接／事务机制。没有新增PHP运行时oracle。

隔离基线 `221cc174eab56fa7e4680372e5fe15dc88b53ee8`。17份代码／测试／三语正文输入与根基线逐字节核对；未叠入同期Fortify及验证器改动、Cargo依赖和锁文件。源码覆盖包与日志保存在 `/private/tmp/openrust-passport-customization-20261120/`。

## 实现与原生映射

Passport增加useAuthCodeModel／useDeviceCodeModel及类型名getter，补齐五类应用内注册。授权码和设备授权全流程、HTTP读取及passport:purge通过类型擦除委托实际选定的Model，保留其casts、主键、观察者、取消、锁与事务身份，不把数据复制到默认表。模型显式连接优先于passport.connection，继而使用协议父连接；同一协议事务跨实际连接明确失败并回滚，包括同一数据库的不同连接别名。

ClientModel增加authCodesFor::<A>()；默认关系保持静态默认返回类型，自定义目标由泛型显式选择。编译模型须自行保留基础casts、key、hidden／guarded、timestamps并提供schema；注册不自动修复自定义模型序列化。应在首次解析相关仓储前选择模型。

ignoreRoutes后允许应用显式注册公开控制器；自定义token路由移除web中间件并保留throttle。默认与自定义prefix的包路由均关闭。既有UserProvider与模型钩子、资源认证和多应用状态继续沿正式入口验证。

生成应用先装Passport后装Sanctum，默认Cargo features、User双trait与两种认证模型注册、独立迁移及已有API路由均验证。两个包HasApiTokens使用UFCS；凭据、Guard、存储与撤销相互隔离，API默认与多级prefix均通过。

## 实际验收

Rust1.94.0、统一MySQL8.4.13。117个不同Passport普通函数、3项显式Chromium、configured guard2、Session MySQL1及扩展5、Sanctum MySQL1、API安装回归1、生成应用共存1，共131个不同函数。四项新增定制函数覆盖五模型真实签发／刷新／设备批准消费／资源／清理、typed关系、观察者取消／失败回滚及重试、缺表和跨连接错误、独立用户provider、自定义路由及双应用隔离。

生成应用保留完整安装正反例、实际PTY／迁移／重复／force／错误重试，并验证Passport JWT与Sanctum personal token分别成功、交叉401、Sanctum撤销后401且Passport仍成功、后续Passport撤销及机器身份边界；默认与/api/admin/两前缀。

严格workspace／all-targets features pail,passport,sanctum Clippy与fmt通过；三语26章节构建、12项通用与6项Passport浏览器检查通过。根原始./artisan、三feature实际HTTP／OAuth错误／no-store／SIGINT及端口释放通过。

保留失败记录：初次定制fixture未保留UUID主键及Client仓储过早解析已修正，路由web组CSRF误挡已修正；生成应用前缀过长及探针缺Container trait、误用关系删除方法已修正。多组测试在数据库连接阶段同时失败，改用新实例串行复验通过，不归因为产品缺陷。resume脚本末尾误用--ignored导致生成应用0项，未计入通过数；随后单独完整执行通过。非MySQL数据库、Windows实机、真实第三方供应商和新增PHPoracle未执行。

## 收尾

只核定本批本地提交，不推送。保留同期Fortify／验证器等协作者改动。源码覆盖包、成功生成应用源码和证据保留后，按归属清理闲置专属source／target／generated／generated-target；磁盘与全部数据卷前后见cleanup.json，保留共享缓存、镜像和全部卷。继续API-S01及其后依赖单元，不把本批结束视为整个第一代开发完成。
