# API-F01 注册与账户更新

2026-11-20；本批完成注册、资料更新与密码更新，API-F01 仍待密码重置、邮件验证、安装与三语正文整体验收。

## 来源与实现

固定 Fortify 1.43.0（`3db382a3d5d6a0d537de32a190ce3b945e621713`）的三个账户动作契约、RegisteredUser／ProfileInformation／Password 控制器、响应与 PasswordUpdatedViaController 事件。沿用此前读取的本地 `v0.x`（`6d1a81924451e15f595f68c8dfb520ad6d6c44cf`）Fortify 账户／管理实现经验；当前公开契约按固定上游重建，不移入旧 authority 协议。

- 正式 GET／POST register、PUT user/profile-information 与 user/password；保持功能开关、guest／auth、guard、前缀和 RoutePath。视图使用原生 Blade 或异步响应回调，响应契约可由容器替换。
- 注册依次规范用户名、应用动作验证／保存、Registered、guard.login、Session regenerate、RegisterResponse。JSON 201，Web intended；remember 走原生 Session Guard。
- 资料与密码更新由应用动作负责验证和持久化；更新对象装回当前 Guard，供响应读取。密码保存后调用 fortify.passwords 指定原生 broker 删除令牌，再派发 PasswordUpdatedViaController；删除失败保留已保存密码且不派发成功事件。
- Rust 用 `Box<dyn contracts::User>` 返回拥有的更新对象，避免通过共享 Arc 可变借用。Clone 保留真实类型供 Registered 事件快照，登录使用原对象；监听器不能像 PHP 一样通过同一对象引用改变稍后登录对象。该约束是所有权适配，不宣称共享可变事件引用等价。
- `createUsersUsing`／`updateUserProfileInformationUsing`／`updateUserPasswordsUsing` 保存应用级动作实例，控制器优先使用它们；未设置时解析相应容器契约。与 PHP 直接 singleton 注册不同，混用时 Fortify 实例优先，容器后续重绑不会覆盖该实例。没有全局动作注册表。
- 默认未实现功能继续启动拒绝，首个为 reset-passwords。本批没有提供安装器默认动作，也没有增加框架 email／confirmed／current_password 验证规则。测试应用的 example.test 邮箱限制和密码确认检查只是夹具动作政策；基础 required／string／min／max／unique、错误包、数据库和 bcrypt 使用正式框架。

## 验收

独立 git archive 基线 `190ae291dca197a6daa1e1c19eb2ede309969e1b`，覆盖本批文件，证据目录 `/private/tmp/openrust-fortify-accounts-20261120/`。同期 Passport 后续提交保留在主线，没有叠加进该快照；最终组合另由 V17 复验。

- 新增四个真实 TCP／MySQL 8.4.13 测试：注册与验证通知、remember、资料更新、密码错误包、命名与默认密码仓库隔离、旧密码失败／新密码成功；容器动作、替换视图与响应、Web intended／status、配置路径、views=false、CSRF；缺失 broker 表和 BEFORE DELETE 触发器拒绝时的持久化及事件顺序。
- [固定 PHP 对照](fixtures/accounts.php)直接执行原三个控制器及响应、契约和事件，四步结果逐项与真实 Rust HTTP／MySQL 比较。PHP 替代 Request／Session／应用动作／Guard／broker／事件边界，不执行完整 Illuminate 验证、SQL 或密码学。原始十份源码 SHA 门禁；输出 SHA256 `6dda232d6a500de27a30782520a7d77f95056535b93de9f258c7b9c3fe4af2f3`，重复生成一致，修改上游源码负样本被拒绝。
- 整包 Fortify 39 个不同测试函数及原生 Session／验证／事件 12 个函数通过。Rust 1.94.0，workspace／all-targets、features pail,passport,fortify 严格 Clippy、全仓格式检查；根 main 实际 HTTP／Pail／恢复／超时清理／SIGINT／端口释放与独立 Artisan 验收见 regressions.log。
- 61 份编译、测试和固定对照输入与独立快照 SHA 一致，见 [输入清单](source-sha256-accounts.json)。首次夹具引用尚不支持的字符串规则、MessageBag 方法名等失败日志保留；修正后重新执行，不把初次失败列为通过。
- 本批未改产品站点，三语正文和浏览器验收随 API-F01 完整交付一起完成；没有运行 SQLite、其他数据库或发布全矩阵。

## 复现

```sh
python3 -B .agents/project/audits/api-f01/fixtures/prepare_accounts.py /private/tmp/fortify-reference /private/tmp/accounts.json
```

在独立源码快照并指定专属 target 执行：

```sh
python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test --offline --locked -p openrust-fortify --tests
cargo +1.94.0 clippy --offline --workspace --all-targets --locked --features pail,passport,fortify -- -D warnings
cargo +1.94.0 fmt --all --check
```

## 收尾

专属 target 闲置核归属后直接删除，逻辑 6.420 GB，df 增加 6.130 GB；Available 62,291,268 KiB，即 63.786 GB／59.406 GiB。Docker 数据卷前后一致；保留全部源码／证据／共享缓存／镜像及数据卷，见 [清理记录](cleanup-accounts.json)。核范围本地提交，不推送。
