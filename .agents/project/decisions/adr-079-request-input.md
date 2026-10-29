# ADR-079：统一请求输入与默认规范化

- 状态：已接受；C04-B1 已按范围验收
- 日期：2026-10-29
- 对应：L019.03 输入基础、L019.04；完整行及其他请求能力不因此关闭

## 决策

公开 body-owning `openrust::http::Request`，内部复用 RequestInfo 与唯一 query/json/form bags。RequestInfo 仍为 parts-only；完整 Request 与 Json 不并列占用正文，Request 提供 deserialize_json / deserialize_query / deserialize_input 从已解析输入转换。原生 Axum Request/Json/Query 是明确逃生口，不应用受管理输入规范化。

1. Request 保留有界原始 Bytes，通过 content() 读取原文；规范化和 merge/replace 只改 bags，不重编码原文。正文消费遵循 Axum 受限读取（默认2MiB及显式 DefaultBodyLimit）；query额外上限16KiB（低于http::Uri自身长度上限，真实HTTP可触达并验收），URL编码输入先限制原始非空字段对1024，再按每个容器entry（含中间容器）计1024、根深度0且子级最大64层、输入展开键按完整dot路径UTF-8字节累计20MiB，超限显式拒绝，不截断。字段、层数与展开限额是常驻进程资源适配，错误不回显内容。
2. JSON内容类型优先，包括GET/HEAD；非JSON的GET/HEAD使用query，其他方法使用form。input/all 使用当前source与query顶层左并集，显式null遮住query，不深合并。无form类型的其他正文只保留raw、form为空；multipart同样只在应用显式选择Request时有界读取raw，本簇不解析其文本/文件bag；文件合并由后继簇完成。全局middleware不缓存上传，现有原生流式入口保持独立。
3. 动态bags使用保序serde_json::Value；typed Json规范化仅替换原始有效JSON的值字符串token，数值词法保持原样，避免Value往返损失Rust u128/i128精度。使用原Axum媒体类型、受限读取和严格typed解析；动态bags由既有值树校验；typed JSON先用IgnoredAny验证语法，再由有界词法遍历累计entry（含重复key）与展开路径并复用同一规范化规则，不改keys或原始Request正文。全局arbitrary_precision实验会破坏应用Figment Serialized(JSON Value)来源，未采用，实验patch保全而不进入核心。PHP连续0..n整数键映射array，稀疏/负数/混合键映射object；删除数组项不重编号，必要时转object。保持数字字符串键规则、后写覆盖、bracket/[]追加、顶层点空格转下划线、标量/数组冲突与PHP的次序；不将url pair decoder当作完整PHP表单解析器。PHP任意字节字符串无法由Rust String/Value完整表示，query/form解码后非法UTF-8稳定400 input_encoding_invalid，不静默替换U+FFFD；畸形百分号按PHP保留字面，合法编码U+FFFD不误拒绝。无法完全表示的PHP键身份按具体案例记录差异，不宣称仅Value即全部等价。
4. input/json 返回 Result<Option<Value>>，以None表示缺失、Err表示输入路径/输出资源超限；支持dot、数字索引和读取通配；query/post只取直接键，null按缺省处理。all、keys、has/has_any/missing、filled/is_not_filled/any_filled、only/except、merge/merge_if_missing/replace按固定上游；has计入null，filled对false/0/空数组为真。only/except 返回 Result<Value>，merge/merge_if_missing/replace 返回 Result<&mut Self>，失败不留下部分修改；merge中的星号为字面键，replace仅换当前source，query fallback保留；修改不重新规范化。
5. 默认先TrimStrings再ConvertEmptyStringsToNull，递归query及选定body bag；固定上游Unicode trim集合，密码排除仅完整key current_password/password/password_confirmation，嵌套user.password仍trim，空密码仍转null。公开 InputNormalization（同时实现Tower Layer）可按路由/应用配置trim、null转换及trim通配排除，也可在请求extensions中覆盖以实现条件跳过；不建立新Hooks或全局可变表。
6. Request、Json<T>、Query<T>/Validated共享内部解析和规范化。typed Query保留字符串到数字/布尔的反序列化能力，null支持Option，嵌套容器与新PHP键语义有对应测试，不能直接from_value破坏数值查询。Json typed继续严格serde类型及现有400/415/422/413错误职责；JSON深度扫描在解码前校验，避免将parser递归资源错误当普通语法；typed入口只预校验语法与有界词法，不要求未知字段的数字可表示为Value，目标类型由最后严格解析决定；Request动态JSON按Laravel解码：畸形JSON为空bag，合法scalar按PHP(array)映射，不改变raw。这一差异在API说明和测试明确区分。
7. Request的公开基础读取采用Rust Option/Value，不模仿PHP隐式转换；额外string/integer/float/boolean/date/enum/enums/array/collect、条件回调、clamp/interval/fluent按固定上游后继C04-B2逐项实现，不以Deserialize关闭该范围。Cookie/Session/flash/old input、FormRequest、multipart文本/文件合并、UploadedFile、CSRF与响应/重定向保留完整后继职责。

## C04-B2 已定实现边界

1. 固定 `InteractsWithData` 的25个公开具体方法，B1覆盖8项；B2另补17项及 `InteractsWithInput::fluent`，保留 exists/str/whenEnum/clamp/interval，不以 Deserialize 关闭动态便捷转换。
2. string/str返回真正的 `support::Stringable`，保 value/消费式trim及Rust借用、所有权、显示和序列化；全套Stringable工具箱仍归后续支持能力。integer/float/boolean保PHP宽松cast/filter，missing采用显式默认而null继续cast；纯标量转换不产生新的validation错误，既有path/resource错误继续Result传播。float可产生非有限值，其HTTP序列化是独立响应职责。
3. Rust无默认参数或方法重载：array/collect/fluent采用key、`*_all()`、`*_only(keys)`三个对应入口，覆盖全部上游选择语义。Collection保PHP键、顺序和稀疏性；Fluent是有界属性快照，和Request复用同一个data/cast内核，不建立第二套输入仓储。
4. support位于http的下层：既有data/limits与固定trim判定共享一次，Request保唯一正文所有权。新返回对象不自动关闭Laravel Collection/Fluent/Stringable全工具箱。
5. 主条件callback的PHP `?:` 和default直接返回须分开；false/0/空字符串/字符串0/空数组/null在主分支回自身，default结果原样保留。Rust以显式结果分支表达，支持对象即使为空也始终truthy；精确公开回调/枚举签名在固定源码和PHP弱类型golden核实后集成，不引入平行Hooks。
6. date/interval继续属于B2完整范围：Carbon宽松解析、PHP格式、默认clock/timezone/IANA DST、年月与原始区间分量均有实现和证据责任，不能以chrono、严格ISO或Duration替代后宣称完成。日历子契约已定，实现与接线仍在进行，B2保持未完成。
7. backed enum以业务实现的trait映射字符串或整数backing，读取保留PHP弱类型联合参数转换，而非直接复用integer/string cast；未知合法backing返回None或显式默认，容器等不合法类型返回明确输入错误。`r#enum`与`enum_value`共享实现；`enums`保键、顺序与重复值。Rust编译期不接受无backing枚举，是静态类型适配，不能冒称PHP运行时行为完全相同。
8. clamp保留整数/浮点标签及原参数顺序，按`min(max(number, min), max)`执行；反转上下界不panic，NaN和相等边界的类型选择须对照PHP结果，不能用Rust的`clamp`或普通排序替代。数字判定采用完整`is_numeric`语义，不采用宽松数字前缀cast。
9. 参考语料使用独立PHP 8.5.0 CLI生成，不作为生产依赖或默认测试前提；提交的合成fixture记录源码版本、校验和与生成命令，由Rust测试读取。617项标量语料包含613项实际serde值路径、3项非有限浮点内核转换及1项JSON数字溢出解码差异；PHP将`1e400`解码为INF而serde拒绝，须独立记录，不能通过修改动态bag或略过断言掩盖。该定向证据不代替B2完整验收。
10. Collection的JSON转换与直接序列化边界拒绝非有限浮点，不能将其静默写为null；内部标量和clamp仍可保留INF/NaN。此错误属于应用值无法序列化，沿用既有internal错误路径，不改成输入cast校验400。私有serializer适配一次实际序列化调用链并覆盖嵌套值，不能用先探测再序列化的两次调用规避有状态Serialize行为。值树预算当前仍在序列化后校验，不声称任意可信应用自定义Serialize都已具备分配前预算。

### 日历类型、上下文与解析职责

- `CalendarInstant`以有符号64位epoch秒和微秒表示时间点，`InputDate`同时保留时区身份，civil年份不受chrono范围约束；转为chrono必须显式可失败。PHP可接受的扩展年份不能因所选Rust库的范围较窄而被归类为上游非法日期。
- `DateOptions`保PHP格式与显式时区，时区支持名称和整数小时偏移。输入文本自带时区按PHP规则覆盖选项；其余依次使用显式选项、请求上下文、应用默认。`app.timezone`默认UTC，启动时解析并安全报告配置错误；独立提取器无上下文时使用UTC。
- `CalendarContext`持有已解析时区及`Arc<dyn CalendarClock>`，复用现有应用装配与请求extensions注入；`SystemClock`和可替换的`FixedClock`不使用进程全局可变状态。每次date调用只读取一次clock，不在提取Request时冻结所有后续调用的当前时间。
- Carbon的test-now模式与无mock生产解析存在时区、缺项和微秒差异，不能将固定时钟语料直接视作生产证明。`CalendarClock::is_test_clock()`默认false，SystemClock使用生产规则，FixedClock明确对应Carbon test-now并返回true；自定义clock显式选择模式。两种模式分别保留真实参考与公开入口测试，不使用全局setter或编译期test条件暗改语义。
- 默认解析与PHP格式scanner共用civil/时区内核；保留宽松日期溢出、相对日期及DST转换。gap按跳变前偏移向前归一；fold按固定timelib偏移推导处理，不统一取首次：真实PHP样本中纽约取前一次，柏林、伦敦和悉尼取后一次，须保留多区差分测试。格式中的`!`、`|`、`U`、缺项和warning分别处理，不把所有warning转成错误。语法与组合按固定timelib源码对应，有限golden不代替完整语法验收。
- `InputInterval`保年月日时分秒、带符号微秒、独立invert及可选total_days；不自动cascade，90分钟仍为90分钟。human单位别名与显式Unit参数的名称解析分开，None与非法输入错误分开；日期和区间不另建平行输入仓储。
- missing/empty先于格式、时区或单位的语法解析，资源预算仍可提前检查。日历解析新增源字符串与显式参数合计20MiB的工作预算；既有20MiB值树限制统计的是展开键，不能作为值字符串已受限的证据。算术checked、扫描受界、错误不回显输入或参数；具体错误分类及边界须有公开入口测试。
- 相对日期的嵌套修饰使用显式状态栈，另限制每次解析最多1024项操作，超限返回413，不递归消耗进程栈。该操作限额是新增常驻进程资源适配，不是Laravel默认值或既有输入entry限额；需覆盖限额两侧、错误脱敏及线性扫描，仍未完成实际验收。
- 聚合日期状态机统一按接受的一次语义解析操作计数，跨分支不重置；完整first-day-of前缀算1项，now/noop也算1项，不按空格或英文单词个数计数。因此1023项前缀加now达到1024，1024项前缀加now达到1025而拒绝。此定义细化替代早期只计前缀的资源测试，不修改PHP语义参考断言；状态机接入与边界仍须实际执行。
- 区间解析也新增每次最多1024项term操作预算，human、无空格compact和relative消费路径共用本次计数；超过预算映射既有Limit/413，不以数空格代替实际词项计数。此适配依据静态复杂度约束，尚无耗时攻击测试证据，不将其描述为已复现的性能故障；边界与错误安全须实际验收。
- 默认parser比较样本为Carbon 3.8.4（Laravel声明的最低版本，不是resolved锁定版本）、PHP 8.5.0与其tzdb。当前268项真实参考来自原Laravel trait加最小direct-key容器及真实Carbon，不是完整Laravel Request/DI验收；生产调用、生成应用、系统/固定clock和完整语法仍须独立验证。

## 实现与验收

内部值树/路径操作与form解析、typed反序列化/Request提取器分工，单一输入模型，不生成平行内核。固定行为单测覆盖源优先级、query/null/深层覆盖、通配、星号写入、稀疏混合key、bracket冲突/重复、原文、密码与Unicode规范化、typed数值/Option及取消/资源限制；模块业务、生成consumer和真实TCP同步。完整workspace/feature/MSRV/deny门禁、不同模型独审与资源收尾通过后限定提交。

## 固定来源

- [Laravel Request v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Request.php)
- [InteractsWithInput](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Concerns/InteractsWithInput.php)
- [InteractsWithData](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Support/Traits/InteractsWithData.php)
- [TransformsRequest](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Foundation/Http/Middleware/TransformsRequest.php)、[TrimStrings](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Foundation/Http/Middleware/TrimStrings.php)、[ConvertEmptyStringsToNull](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Foundation/Http/Middleware/ConvertEmptyStringsToNull.php)
- [Symfony Request v8.0.0](https://github.com/symfony/http-foundation/blob/v8.0.0/Request.php)、[PHP注册变量语义补充](https://github.com/php/php-src/blob/php-8.5.0/main/php_variables.c)

- [Laravel Arr v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Collections/Arr.php)、[data_get helpers](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Collections/helpers.php)
