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

## 实现与验收

内部值树/路径操作与form解析、typed反序列化/Request提取器分工，单一输入模型，不生成平行内核。固定行为单测覆盖源优先级、query/null/深层覆盖、通配、星号写入、稀疏混合key、bracket冲突/重复、原文、密码与Unicode规范化、typed数值/Option及取消/资源限制；模块业务、生成consumer和真实TCP同步。完整workspace/feature/MSRV/deny门禁、不同模型独审与资源收尾通过后限定提交。

## 固定来源

- [Laravel Request v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Request.php)
- [InteractsWithInput](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Concerns/InteractsWithInput.php)
- [InteractsWithData](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Support/Traits/InteractsWithData.php)
- [TransformsRequest](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Foundation/Http/Middleware/TransformsRequest.php)、[TrimStrings](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Foundation/Http/Middleware/TrimStrings.php)、[ConvertEmptyStringsToNull](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Foundation/Http/Middleware/ConvertEmptyStringsToNull.php)
- [Symfony Request v8.0.0](https://github.com/symfony/http-foundation/blob/v8.0.0/Request.php)、[PHP注册变量语义补充](https://github.com/php/php-src/blob/php-8.5.0/main/php_variables.c)

- [Laravel Arr v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Collections/Arr.php)、[data_get helpers](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Collections/helpers.php)
