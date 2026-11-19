# Passport-05 设备授权

2026-11-20：本单元骨架、实现及行为验收闭合。V06 仍部分验收，继续 Passport-06 至 09；不称 Passport 整包或发布完成。

## 固定参考与复用

固定 Passport 13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7`，League oauth2-server 9.4.1 `9d2f6fc0a0b5aa1bb02506971d3a4ecff2c6526c`。核对 DeviceCode、Bridge DeviceCode／Repository、六个设备控制器／Session trait、批准拒绝响应、路由、Provider／Passport、Feature DeviceAuthorizationGrantTest，以及 League DeviceCodeGrant／Response／测试。18 份下载源码 SHA 见 upstream-sha256.json。本批未新增 PHP 运行时 oracle，不把源码核对描述为 PHP 运行对照。

只读检查同级 openrust-v0.x `6d1a81924`，旧版只有设备 grant 名称和客户端状态，没有可复用设备协议实现；本批按固定源码接入主线模型、事务、Session 和 URL 生成器。隔离快照基线 `7e70e978a5ff08293c8931aba7fad08c5c5b1967`，18 份本批源码／测试／正文 SHA 与接受副本一致；同期 Fortify 等协作者 WIP 不计入验收。

## 实现及 Rust 映射

- 设备 grant 默认 true；路由装配前关闭则不注册设备端点。POST /oauth/device/code、GET /oauth/device、GET／POST／DELETE /oauth/device/authorize 按真实命名 URL 和 passport.path 生成。兑换还要求 passport.device 命名路由存在，ignoreRoutes 不能仅凭开关启用设备协议。
- DeviceCode 是公开真实 Eloquent 模型，使用字符串主键、scope／boolean／datetime casts 和 Passport 连接回退。动态替换 DeviceCode 模型仍属 Passport-09 范围，不在此批声称可用。
- 设备码 80 位 hex、用户码 8 位固定 20 辅音字母。OsRng 有错误传播，拒绝 >=240 的采样后取模，避免偏差；真实唯一冲突最多尝试 10 次，Eloquent 取消不当成功。
- creation 只识别客户端，不验证秘密，遵循固定 League；token 兑换验证机密秘密。TTL600 秒，interval5 秒；pending／slow_down 保存 last_polled_at 后提交，再返回 OAuth400。只对待决定状态限速，批准后可兑换；slow_down 文案要求客户端放慢，但服务端固定间隔仍为5，与固定代码相同。
- GET 输入仅移除连字符，不自动转大写；错误返回 old input 与 MessageBag。应用通过 deviceUserCodeView 和 deviceAuthorizationView 提供编译回调；缺回调显式500。Session 保存原生 code／user 和一次性 nonce，批准拒绝保持实际认证、Session／CSRF、status flash 与重定向。
- Rust 的事务行锁额外防止两个已打开同意页的浏览器重新绑定已决定设备码；竞争只允许一个成功，凭据 subject 属于赢家。这是明确的原生加强，测试覆盖。complete 与固定上游同样不重新检查 expiry；poll 检查过期后才可签发。
- 批准后访问／刷新签发与设备撤销属于同一事务，事件 veto 或数据库故障回滚；单次兑换、客户端及应用隔离成立。device grant 保留 *，authcode／refresh 删除 *，与固定 ScopeRepository 的 grant 分支一致。

## 实际验收

证据目录 `/private/tmp/openrust-passport-device-20261120/`，保留全部脚本、失败及成功日志、源码覆盖包和 SHA。

- Rust 1.94.0；tests/mysql_acceptance.py 验证实际 MySQL 8.4.13 后执行。完整定向回归 regression.log 为92个 Passport 函数；新增命名路由门禁后 mysql-final3.log 的29项通过，再加显式执行的 Chromium 函数，共94个不同 Passport 函数。另有2项 configured guard、5项 Session extension、1项设置专用环境变量后真正执行 SQL 的 Session MySQL，共102个不同函数。
- 11个新增普通设备测试覆盖公开／机密、HTTP Basic／form、默认／显式scope、pending／slow_down／last poll、CSRF／Session／nonce与重放、拒绝／过期／客户端绑定、用户批准竞争、兑换竞争、事件回滚、真实唯一冲突和十次耗尽、关闭／多级前缀／ignoreRoutes门禁、通配scope、表故障及双应用隔离。29项中其余18项为已有授权码／生命周期路径。
- browser-final.log 显式运行 ignored 的 Chromium 测试，真实输入带连字符用户码、登录、点击批准 form、设备轮询、实际资源认证和重放拒绝通过；未将默认忽略当作验收。
- clippy-final.log：最低工具链完整 workspace／all-targets features pail,passport 严格 Clippy；fmt-final.log为空。未运行非MySQL数据库测试。
- 三语15个h2正文同步，构建通过；根挂载与 /docs/ 子挂载12项通用＋6项Passport浏览器断言通过。
- 根实际二进制 HTTP /、/up、API／Web／Accept JSON404、OAuth unsupported_grant_type400及no-store、Artisan inspire、原始 ./artisan inspire、keys／purge帮助、SIGINT与端口释放通过。原始artisan后重建features再验证OAuth端点。
- 失败日志保留：最初关闭开关在boot之后导致路由仍存在，修正装配时序；新增自定义web端点须携带CSRF后才验证到OAuth门禁。根smoke导入与Python标准库同名的临时http.py，触发重复生成，已重命名initial-http.py、修复重复与被覆写控制器；全部受影响设备、Chromium、严格Clippy、格式和根入口再次通过。共享根没有受此临时脚本影响。

## 收尾

仅本批范围核对、本地提交，不推送；保留协作者全部在途改动。专属闲置source／target／站点构建产物直接清理，源码已保留于Git与覆盖包，实际释放与Available见cleanup.json。保留共享缓存与全部数据库卷；数据库临时容器由统一入口按标签回收。后续PAT／机器令牌／scope中间件／SPA Cookie／事件、显式可选grant、安装和共存继续开发。
