# ADR-086：同一配置仓储的原始读写

状态：原始读取已完成main组合验收；R2-R01运行期原始写入已实现并通过限定验收，资源收尾通过，本地提交见Git；完整Config仍未闭合。

## Spec

1. 固定 Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的 `src/Illuminate/Config/Repository.php`、`src/Illuminate/Collections/Arr.php` 与 `tests/Config/RepositoryTest.php` 为来源；派发中的 `ConfigRepositoryTest.php` 路径不存在，以实际树更正。
2. 在现有 `ConfigRepository` 增加 `get(&str) -> Result<Option<Value>, ConfigError>`、`get_or(&str, Value) -> Result<Value, ConfigError>`、`has(&str) -> Result<bool, ConfigError>`。
3. 增加 `get_many(&[&str]) -> Result<Vec<(String, Value)>, ConfigError>` 与 `get_many_with_defaults(&[(&str, Value)]) -> Result<Vec<(String, Value)>, ConfigError>`；无默认的缺失值为 null，显式 null 不用默认。
4. 批量保留首次键出现顺序，重复键以后一次结果覆盖，符合 PHP 关联数组；Vec 避免 JSON map 排序改变输入顺序。
5. `all() -> Result<Value, ConfigError>` 返回当前已发布独立快照；以独立方法适配上游 null key 返回全部，不把空字符串当 null。
6. 完整字面键优先，然后按点逐段查找；不对中间剩余路径重新作字面匹配。支持空键与规范数字索引；`01` 不等于数组索引 1。
7. 复用 `snapshot()` 的原子 Figment 克隆及既有 `safe_source_error`；原始读取错误忽略来源提供的路径，类型化读取仅保留 schema 声明的诊断字段。不以 Figment dotted lookup 实现 Laravel 查询。本基线 support 无公开 Arr，故只加仓储私有值查找函数，不建立第二 Arr。
8. 每次批量操作只取一次快照并完整反序列化，来源失败整体返回脱敏错误，不返回部分结果；成功值含秘密，输出由调用方脱敏。
9. 不改 env、配置来源、默认/合并结果、版本、freeze 与 Provider 生命周期；仅收敛相邻 schema 与类型化错误的路径显示；不增依赖，不实现动态写入、typed helper 或 collection。
10. 单元覆盖 null/缺失/false/0/空、literal dotted key、数字索引、批量默认与重复、快照独立、失败脱敏及并发同快照；分支最小 feature 具名门禁已执行，完整 feature 组合仍待总控集成验收。
11. 唯一示例已接入固定 `example.greeting` 业务读取；独立 consumer 的公开 Provider fixture 已在新生成应用安装并通过具名测试与本地 TCP。此子簇不关闭完整 Config 或 L005。

## 安全诊断适配

原始读取与相邻的类型化配置校验共用错误边界。schema 校验只在错误路径显示已声明对象字段；未知对象键停留在已知父路径，Map 动态名称以固定 `<entry>` 表示，Sensitive 包装层同样执行该规则。类型化来源路径按真实 schema 层级逐段核对；来源提供的规范数字数组段也只显示固定 `<index>`，非规范数组段在已知父路径停止，来源的未知片段不回显。schema 自行校验数组时生成的索引仍可用于可信诊断。此诊断收敛是 OpenRust 的安全增强，不宣称 Laravel 默认错误文本等同。成功读取的原始值仍由调用方负责控制输出。

## 固定来源

- [Repository](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Config/Repository.php)
- [Arr](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Collections/Arr.php)
- [RepositoryTest](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Config/RepositoryTest.php)


## R2-R01：核心运行期原始写入

状态：实现及限定行为、工作区门禁、生成/MySQL/真实TCP验收通过；证据见 [测试基线](../memory/testing.md#r2-r01-config-runtime)。来源初始键序及本ADR列明的完整Config缺口继续保留。

1. 固定来源仍为本ADR的Repository/Arr及Config测试；在同一ConfigRepository提供 `set(&self, key: &str, value: Value)` 与按迭代顺序的 `set_many`，沿既有Result/安全错误，不增加依赖、进程全局Facade、第二仓储或管理平台。
2. 写入按Arr::set逐段处理dot路径，中间scalar/null可替换为数组；尾项整体替换而非递归overlay。空键、空dot段、规范整数、稀疏/混合数组和覆盖原位置须有固定PHP实证；get整键字面优先保持，写入不按literal优先，批量不得BTreeMap重排。
3. 允许在Register/Boot/Frozen阶段调用核心原始set；freeze继续最终启动校验并封闭注册/boot overlay，原阶段错误保留。启动前set仍在启动时接受已注册schema校验；启动后raw set不套用启动schema拒绝Laravel可存值，typed读取失败按Result脱敏报告。
4. 同一App各句柄共享，跨App独立；批量先构造完整候选再一次发布，读者只见旧或新。空批次不改版本，非空成功一次递增；来源失败、版本耗尽或并发冲突不发布部分数据。不自动隐藏重试，保留调用边界。
5. 有序输入及原始值中的键顺序、数组形态不能因Figment字典投影而丢失。若需调整内部表示，权威值与Figment类型化/来源元数据投影必须在同一个State/版本中成对原子发布；不能让原始/类型读取落到两个独立可变配置源，也不能以JSON限制排除可表达的混合或稀疏数组。
6. App与AppState公开 `config_repository()` 返回同一服务句柄；App.section读取live仓储。Provider/Resolver复用既有句柄，不重建仓储。App.config/figment保留时须明确为启动快照；既解析typed服务、DB/Redis/邮件等资源不因内存set自动重建。
7. Clone仍为Rust共享服务句柄；独立快照不是Clone，上游PHP对象clone数组复制语义差异明确测试/记录。不增加未经核心来源证实的请求局部配置层；同App后续请求可见核心内存写入。
8. 本簇不写.env/文件/DB，不跨节点传播、不提供默认管理HTTP、不实现动态Settings资源刷新；ADR-080的部署密钥及管理员白名单/权限/重启政策保持，可信应用代码的内存set与部署管理分责。
9. 唯一模块沿已有config_reading Provider/服务/路由接线，仅写固定example业务key和常量值，无接受任意key/value的HTTP。验证同App跨请求更新与第二App隔离；生成/公开consumer同步使用同一API，不重做配置系统。
10. 单元承载dot/批量/数组/顺序与失败边界，真实Provider/App/Resolver验证同句柄、启动校验和旧snapshot/typed服务；模块/生成只验证接线。先固定PHP对应输出，再实际执行适用workspace、生成与真实HTTP验收，不用旧读取证据关闭新set。
11. PHP对象/Closure/资源与Rust Value的表示差异须明确分类，未实现项保持开放；prepend/push及其他读取快捷方法另列后继，固定Config Repository无内置pull，不凭集合同名方法新增。signed后继才加入key字段/resolver与URL签名，不夹入本簇。

### 来源初始顺序的现存缺口

现有 `ConfigRepository::new(Figment)` 接口接收的 Figment 字典已使用排序映射，无法从该值恢复进入 Figment 前的来源对象插入序。这是现有来源入口的表示缺口，不是 Laravel 行为，也不能据此宣称完整 Config 顺序等价；后继须沿配置加载入口保留来源顺序。本簇必须保留运行期写入的有序值，并验证 Register 写入经过启动默认值、Boot overlay 和 freeze 后仍保持有效键序及稀疏/混合数组形态。同步类型化投影不得复活已替换或删除的旧值，也不得掩盖来源错误。

## R2-R02：有序来源进入同一仓储

状态：实现已静态交接，首轮142项定向测试及fmt通过；核心独审、适用Rust门禁、生成/MySQL/真实TCP及资源收尾通过，见[本簇验收](../memory/testing.md#r2-r02-config-sources)。前置R2-R01本地提交 `2de050f`。

1. 固定同一Laravel基线的LoadConfiguration、Repository、Arr与测试，先取得文件自然排序、首次插入/覆盖位置及合并的实际对应证据；不把保留现有错误行为当成等价目标，未闭合合并差异如实分类。
2. 提供不可变 `ConfigSources`，私有地携带同一次来源解析得到的有序原始值与Figment投影；最终仍只有一个ConfigRepository State/锁/版本，不建立旁路可变配置源。Debug不暴露原始值或来源秘密，可不实现或仅提供安全摘要。
3. `ConfigLoader::sources()` 构造该输入；默认值、文件及环境取同次捕获数据，保留来源metadata/profile、优先级、安全错误和失败不发布。环境分项连接、通用及命名空间alias/null都用同一捕获集，不以每层分别读取后各自配对冒称单次快照；非UTF8环境不panic。根引导一次获取sources，应用typed配置与Provider启动复用它，不分别重读。
4. 提供fallible `ConfigSources::from_ordered_entries` 显式有序输入；`figment()`为只读投影访问。既有Figment可通过显式转换接入，但无法恢复已丢失的顺序，须标明有损，不冒称严格对应；构造不得用raw掩盖Figment来源错误。
5. 既有 `ConfigRepository::new`、`ProviderBuilder::start`、`TestApp::builder` 接收可转换的同一输入，优先沿现方法扩展而不平行增加三组 `_sources` API；必要Send/所有权约束按真实编译收敛，原Figment调用无需机械重写。
6. 有序原值在Register默认补齐、Provider包合并、Boot overlay、freeze及Frozen set中保持。仅按新来源键追加、已存在位置覆盖；默认段及包操作的声明次序也要审计，不从BTreeMap反推。原始/类型化值不一致或来源失败必须安全失败。
7. 文件自然排序和TOML表/数组顺序按固定来源实证；环境映射新增路径取明确稳定顺序，保留别名/命名空间优先级与null语义。环境枚举不具有PHP配置文件声明顺序，不据此捏造相同保证。
8. 优先既有依赖；若需要已有toml的保序feature，由root唯一协调manifest/lock及stack记录，不让worker并改共享清单，不手写TOML解析器或修改进程全局环境实现快照。
9. 唯一模块、公开consumer和生成测试经现Provider展示有序业务来源及生命周期；根默认引导实际走新来源入口，不仅增加可选API。生成验收fixture可在自有fresh应用新增固定 `config/reader.toml`，经真实server的固定业务键顺序路由验证TOML声明序；须拒绝覆盖既有文件，绑定安装变化与后续源码链，不仅证明TestApp显式有序输入。App的live仓储及已解析服务快照边界保持，框架不因来源保序向文件/DB写配置或重建资源。
10. 核心完整顺序/错误矩阵、生产引导/Provider/TestApp、唯一模块/生成/真实HTTP分层验收，MySQL按当前日常政策；类型表示、完整Config与signed仍开放，不虚构本项为签名算法的硬依赖。实现齐备及静态审查后集中验证，不新增测试target或复制每层全部底层矩阵。

来源发现边界：固定LoadConfiguration递归发现配置文件，按“嵌套目录点前缀与basename”的配置键自然排序；现Rust入口仅发现根目录领域TOML，且文件内容显式携带领域表。R2-R02须精确标明所验证的来源顺序，不把平面文件排序等同完整发现/命名映射。递归发现与领域命名映射未闭合时继续作为完整Config待办，不静默改写所有TOML含义，也不把现布局当成永久目标或不适用项。
