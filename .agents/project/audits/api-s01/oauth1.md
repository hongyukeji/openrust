# API-S01 Twitter OAuth1与统一协议入口

2026-11-20，基于独立 `7502cc12d0d10a48acccf2157e3bd24234af488f` 快照，接续 Facebook 批次。API-S01 整体仍未闭合；继续本地身份归属与登录整合，完整 OIDC RP／SSO 属于 API-O01／02。真实 Twitter 授权仍缺测试应用、凭据、注册回调与账户。

## 固定契约与实现

Socialite v5.33.0 `8032bb1fa9d74aae54438f216e4d45db90d27ea9` 的 One/AbstractProvider、TwitterProvider、User；League OAuth1 Client v1.12.0 `aa8fe766f772f9233d6c06f9ef003fd8129e408d` 的 Server、Twitter、HmacSha1Signature、EncodesUrl。League 版本属于 Socialite 声明的 ^1.11 范围，作为本批显式固定参考，未声称为某个上游 lock 的解析结果。旧版 v0.x 无 Socialite 实现，已有旧 auth/oidc.rs 的可复用机制留 API-O01；本批未移植旧公开 API。

新增 one::AbstractProvider／TwitterProvider，使用现有应用 HTTP Factory、真实 Session 和 AWS-LC HMAC_SHA1_FOR_LEGACY_USE_ONLY，不自行实现密码算法。签名沿 League 的 rawurldecode→rawurlencode、顶层编码键排序、递归 query、query／body合并、默认端口和 HMAC key 规则；query 解析复用框架 Request，沿用其长度／UTF8限制。随机 nonce 使用系统加密随机源，每次请求独立生成32位十六进制值；时间戳读取系统时钟。官方四个 Twitter OAuth1 URL及include_email=true保留。受控端点通过 withEndpoints／extendOAuth1 设置，不改变内置定义。

redirect 异步请求临时凭据，严格检查 oauth_callback_confirmed=true 后存 Session oauth.temp；scope 与 OAuth2 scopes 分离，支持配置 services.twitter.scope／TWITTER_SCOPE，或请求内 scope()。scope 空／"0" 不发送 x_auth_access_type。回调先检查两输入存在及 Session，严格核对临时标识后进行远端交换；本地读取而不消费 oauth.temp，远端负责临时凭据单次使用。错误内容不包含供应商响应、token或secret。

Twitter回调映射附加url字段、location、description与avatar_original；userFromTokenAndSecret沿One基类仅保留League extra。tokenSecret作为共同User显式credentials中的可选Zeroizing字符串；OAuth2为None，凭据不实现Debug或Serialize。请求内缓存以实际token／secret对识别，任一变化重新获取。以零化凭据对替代上游SHA1(token+"_"+secret)，避免拼接分隔符歧义；缓存仅在成功取到有效profile后更新，避免失败污染。缓存不放入manager或其他请求。

## Rust公开接口适配

manager／facade／contracts::Factory现返回根Provider协议枚举；共同redirect().await与user().await适用于两协议。OAuth1 redirect先有网络操作，无法维持同步公共契约，因此统一异步；具体two::AbstractProvider仍保留同步redirect。oauth1()／oauth2()消费枚举并取得专有类型，协议不符返回显式Error::Protocol，避免静默忽略专有参数。两枚举分支装箱，避免760字节的公开枚举与大变体差距。既有测试及三语示例全部迁移。

twitter只有严格整数oauth=2走OAuth2，缺失／1／字符串"2"／浮点2.0均保持OAuth1选择；twitter-oauth-2和x原别名回退、OAuth2扩展注册优先级保留。OAuth1扩展注册属于各应用；scope与缓存属于当前请求。OAuth1不提供stateless方法。

## 固定PHP与真实入口验收

使用固定PHP8.5.12镜像 `php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9`，禁网、只读挂载真实源码。实际League签名／Server／Twitter与Socialite One方法运行；HTTP与Illuminate请求／Session为最小适配器，不称完整Laravel服务端执行。真实Guzzle PSR7 2.8.0与PSR HTTP Message 2.0负责URI行为，其SHA／MIT许可证见oauth1-dependencies.json；League也为MIT。未向Rust生产依赖新增包或修改Cargo.lock。

14组真实HMAC签名涵盖UTF8／空值／百分号／加号／重复query／PHP点号键／默认与非默认端口／递归数组及body覆盖；两条用户映射、临时凭据和HTTP字段、10种回调输入／错误顺序、缓存与secret变化形成固定fixture。重复生成逐字一致；修改EncodesUrl参考源码在加载前拒绝。PHP null verifier触发固定源码rawurldecode的deprecated警告，已单独保存stderr，不混入JSON。null／false签名按空字符串计算，实际表单分别省略／发送0；Rust真实HTTP亦逐项验证。框架空字符串转null中间件对应省略表单路径，未用PHPempty错误拒绝空verifier。

新增5个独立Rust测试函数。受控远端核对每个真实Authorization签名、独立随机nonce、时间戳、scope query和表单；真实Kernel／加密Session Cookie覆盖临时凭据、回调映射、跨应用拒绝、远端单次消费及重放拒绝、直接凭据／缓存／secret轮换、拒绝／畸形／撤销、观测脱敏和应用内注册隔离。统一Factory契约和协议错误通过。全Socialite **34个函数**，根纯配置 **1个函数**通过；未将受控HTTP声称为真实Twitter授权，也未将本批根MySQL运行声称为本地用户归属验证。

最低Rust1.94.0：

- `cargo +1.94.0 test --locked -p openrust-socialite`
- `cargo +1.94.0 test --locked --test configuration -- application_configuration_loads_all_sections_and_typed_overrides --exact`
- `cargo +1.94.0 clippy --workspace --all-targets --locked --features pail,passport,socialite -- -D warnings`
- `cargo +1.94.0 fmt --all --check`
- feature根main最终重建，保存单独二进制后，使用`python3 -B tests/mysql_acceptance.py -- python3 -B <stage>/root-final.py`；MySQL8.4.13实际版本核验、根HTTP 200/404与内容类型、保留二进制Artisan inspire／Passport帮助、原始./artisan inspire、SIGINT／端口释放全部通过。
- 三语`npm run build`；check_browser.mjs **12项**、check_socialite.mjs **6项**，均覆盖`/`和`/docs/`，8个h2章节保留，并新增OAuth1公开接口文本检查。

严格门禁基于独立快照，未混入同时在途的Fortify安装／框架／Cargo改动。普通本地提交仅包含本批文件和共享账本本批增量，不推送。

## 证据与清理

证据目录`/private/tmp/openrust-socialite-oauth1-20261120/`。保留固定PHP源码／依赖源码包、fixture、日志、审核patch、SHA manifest与accepted-overlay.tgz。验收完成后核对进程和容器标签，删除本批独立source／target／root-main；全部数据库卷／共享镜像与缓存保留。实测释放和剩余空间见oauth1-cleanup.json。该目录中的早期失败日志保留，包括大枚举Clippy、测试迁移、PHP symlink挂载和null／false表单边界；最终结果以delivery及final日志为准。
