# 05-03 属性转换与模型序列化

## 访问器、修改器和基础转换

- 对照固定 Laravel 的访问器优先级、修改器返回多个列、标量和 JSON 类型转换。参考固定 Goravel 的连接级 ORM Query，转换只在模型层执行。提取旧版 `6d1a8192` 的 `support/scalars.rs::numeric_prefix`，复用 `eloquent/attribute_cast.rs` 和其测试中的数字前缀、字符串布尔和无效 JSON 读取规则。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php) · [Laravel 模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `Model::attribute` 提供类型化访问器和修改器，`Model::casts` 声明布尔、整数、浮点、字符串和 JSON。访问器优先，`fill` 经批量赋值保护后执行修改器或写入转换，所有输入成功后安装属性。原始属性和脏字段快照继续用于持久化。`getAttributeValue` 和 `setAttributeValue` 暴露可失败的转换入口，原有借用式读写保持原始语义。
- SQLite 与真实 MySQL 8.4.13 验证创建、重查、保存、访问器覆盖转换、虚拟属性、数值前缀、布尔、SQL NULL、JSON 对象和标量，以及批量转换失败后的状态。三语示例均经双库执行并验证持久化结果。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、VitePress 构建、12 项通用浏览器组合和 6 项新增章节锚点通过。

05-03 仍在进行。精确小数、日期与时间、查询级和自定义转换、转换后的脏值等价比较、隐藏及追加字段与模型序列化待接通。当前浮点到字符串用 Rust 默认格式，尚未对齐 PHP 全部数值边界。模型序列化文档继续随实现闭合。

## 精确小数与转换后的脏值比较

- 对照固定 Laravel 的 `asDecimal` HalfUp 规则及 `originalIsEquivalent`。提取旧版 `6d1a8192:vendor/openrust/framework/src/database/eloquent/decimal_cast.rs` 的精确解析、有限分数及输入和输出上限，沿用同版 `bigdecimal 0.4.11`。参考 Goravel 的连接级查询职责，模型转换独立于驱动。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `AttributeCast::Decimal(u32)` 将文本读为定标字符串，赋值保留输入。模型 `getDirty`、`isDirty` 和 `originalIsEquivalent` 按转换结果判断，浮点使用固定基线的四倍 epsilon，保存跳过等价列。真实变化仍写入原始存储值。ModelState 的旧快照读写继续体现原始属性。
- SQLite 与真实 MySQL 8.4.13 验证超过二进制浮点精确范围的小数、正负 HalfUp、有限分数、指数上限、整数与 JSON 等价，以及等价保存后的数据库原值。MySQL 使用 `DECIMAL(38,5)`，SQLite 使用文本列保留输入精度。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建、12 项通用浏览器组合和 6 项新增锚点通过。

通用浮点文本边界与 JSON 对象键顺序继续核对。

## 日期、时间与应用时区

- 对照固定 Laravel 的 `asDateTime`、`fromDateTime` 和 `serializeDate`。参考 Goravel 连接级 ORM，提取旧版 `6d1a8192` 的 `eloquent/model_date_cast.rs` 及回归中对存储、读取、Unix 时间与应用时区的处理。 [Laravel 日期转换](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#date-casting) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `Date` 返回应用时区当日零点对应的 UTC RFC 3339 字符串，`DateTime` 返回同一时刻的 UTC 字符串，`Timestamp` 返回 Unix 秒。模型 `dateFormat` 采用 Chrono 格式。带偏移量的输入写入前转到应用时区，比较脏值时使用存储精度。夏令时重复时间取较早时刻，跳过的本地时间返回错误。连接之间独立保存时区，未关联连接的模型使用 UTC。
- SQLite 与真实 MySQL 8.4.13 验证存储与读取、微秒、负分数 Unix 时间、等价偏移、夏令时和跨应用隔离。隔离副本的格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建和 18 项浏览器检查通过。
- 当前接受 RFC 3339、本地日期及时间、整数和小数 Unix 时间。Carbon 的扩展解析、查询级与自定义转换、`serializeDate` 输出定制、字段可见性和模型序列化仍待接通。

## 查询时转换

- 对照固定 Laravel `Builder::withCasts`、`Model::newInstance`、`Collection::fresh` 和 `testNewInstanceReturnsNewInstanceWithMergedCasts`。检查旧版 `6d1a8192` 的 `eloquent/attribute_cast.rs` 及转换用例，沿用已有的原始属性读取、空值旁路与转换机制。查询级覆盖接到当前 Builder，连接职责参考 Goravel。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#query-time-casting) · [Laravel Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php#L1881) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php#L702) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `withCasts` 接收列名与类型化转换，供查询返回和创建的模型使用。最后一次同名配置优先，访问器与修改器仍优先。覆盖规则参与读取、写入与脏值比较，`getRawOriginal` 保留数据库值。Rust 用共享映射持有同查询的转换规则，`mergeCasts` 写入时分离单个模型的配置。Rust 的关联函数 `Model::newInstance` 仍创建独立模型，查询继承通过 Builder 的 `make` 和加载路径完成。
- SQLite 和真实 MySQL 8.4.13 验证日期别名、空值、连续覆盖、访问器优先、等价写入、创建失败后重试、结果间隔离、集合 `fresh` 重查，以及多对多和穿透关系预加载。集合重查沿用第一个模型的转换规则，按新查询返回的列读取数据。
- Rust 1.94.0 隔离副本执行格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire`。另以必需 MySQL 模式复跑关系测试。三语 `selected_date` 示例各在 SQLite 和 MySQL 执行，覆盖空结果与日期读取。VitePress 构建、12 项通用浏览器检查和 6 项查询转换章节锚点通过。

## 枚举与自定义转换

- 对照固定 Laravel `HasAttributes` 的枚举读写、类转换单列和多列赋值、仅入站转换及原始值比较。核对枚举集成测试的空值、非法成员和错误类型用例。提取旧版 `6d1a8192` 的 `eloquent/enum_cast.rs` 中类型检查、成员到存储值的映射及原始值脏比较，以及 `eloquent/class_cast.rs` 中写入补丁、仅入站和自定义比较语义。Goravel 的连接级 ORM 查询职责继续沿用。 [Laravel 枚举转换](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#enum-casting) · [Laravel 自定义转换](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#custom-casts) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php) · [Laravel 枚举测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelEnumCastingTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- Rust 通过 `CastableEnum` 和 `EnumCast::of` 注册类型。`getEnum` 和 `setEnum` 显式传递枚举类型，通用 `Value` 继续表示数据库值。`CastsAttributes` 和 `CastsInboundAttributes` 使用显式注册的共享转换器，支持读写、单列或多列补丁及可选比较器。当前类转换读值使用 `Value`，有类型对象缓存、`Castable` 工厂和自定义序列化仍待实现。
- SQLite 和真实 MySQL 8.4.13 验证成员名称、整数后备值、空值、错误枚举类型、非法成员、修复旧数据中的非法枚举值、多列写入、仅入站读取、比较器与批量赋值失败后的状态保持。
- Rust 1.94.0 隔离副本格式检查、锁定离线工作区 436 项测试、全目标严格 Clippy、`./artisan inspire` 通过。三语示例各在双库执行，核对写入和重查。VitePress 构建、12 项通用浏览器检查和 6 项新增章节锚点通过。

## 加密转换与密钥轮换

- 对照固定 Laravel 的 `HasAttributes` 与加密转换、脏值集成测试。提取旧版 `6d1a8192` 的 `eloquent/native_encrypted_cast.rs` 及 `tests/unit/database/eloquent/native_encrypted_cast.rs` 中空值旁路、标量字符串转换、结构化解码、零化缓冲和旧密钥脏值规则。Goravel 的加密服务由应用容器注册，OpenRust 沿用现有 Encrypter 契约。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#encrypted-casting) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php) · [转换测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelEncryptedCastingTest.php) · [脏值测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelEncryptedDirtyTest.php) · [Goravel Crypt Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/crypt/service_provider.go)
- `EncryptedCast::{String, Array, Json, Object, Collection}` 通过模型读写管线持久化密文。连接弱引用所属容器，按需解析应用加密器。未绑定连接的模型使用当前应用作用域。由应用外部直接构造且未关联容器的连接无法解析加密服务。旧密钥可读，重新赋值用当前密钥。未配置旧密钥时比较解密结果，配置旧密钥时把新生成的密文视作变更。
- Rust 的结构化读取返回 JSON，可通过 `getEncryptedCollection` 读取集合。JSON 对象按键值比较，键顺序与 PHP 关联数组的严格比较存在差异。类转换 `AsEncryptedArrayObject`、`AsEncryptedCollection`、加密 JSON 路径赋值和 Laravel 的静态 `encryptUsing` 入口仍待核对。
- SQLite 和真实 MySQL 8.4.13 验证五种转换、密文原值、空值、错误密钥、缺失密钥、畸形密文、并行应用隔离、旧密钥读取与当前密钥重写。Rust 1.94.0 隔离副本的格式检查、锁定离线工作区 438 项测试、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建及 18 项浏览器检查通过。

## 字段可见性与模型序列化

- 对照固定 Laravel 序列化文档、`HasAttributes`、`HidesAttributes` 和模型测试中的可见性、蛇形关系名、循环关系和追加属性。提取旧版 `6d1a8192` 的 `attribute_store_tests.rs` 追加访问器、`model.rs` 可见交集及 `relations.rs` 已加载关系合并用例。Goravel 的 ORM 查询仍负责取得模型，序列化在模型层处理。 [Laravel 序列化](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-serialization.md) · [Laravel 属性源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php#L211) · [Laravel 模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php#L1376) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- 模型和集合提供 `toArray`、`toJson`。模型另有 `attributesToArray` 和 `relationsToArray`。静态 `hidden`、`visible`、`appends` 由元数据声明，单实例修改按需分配。隐藏过滤先于访问器、转换和关系投影。关系包含已加载的单值、列表、多态和中间表属性，输出键使用蛇形命名。循环重新遇到同一模型时只输出属性。自定义关系可通过 `Relation::serialize` 提供投影。
- Rust 返回可失败的 JSON 映射，键顺序采用 `serde_json::Map`。`serializeDate` 接收 UTC 时间，存储格式独立。自定义类型可投影转换后的值，访问器优先。无效 UTF-8 和非有限数字会返回错误。逐字段日期输出格式、条件可见性和路由自动转换仍待核对。
- SQLite 与真实 MySQL 8.4.13 验证隐藏字段、追加访问器、日期、枚举、自定义输出、隐藏加密字段、关系、空关系和中间表，MySQL 确认序列化新增 SQL 查询数为 0。另验证循环关系和失败后的重试。Rust 1.94.0 隔离工作区格式、锁定离线 439 项测试、严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建及 18 项浏览器检查通过。

## 逐字段日期格式与模型 HTTP 响应

- 对照固定 Laravel 的逐字段日期格式、`Router::toResponse` 模型状态分支，以及日期归零和 JSON 路由测试。提取旧版 `6d1a8192` 的 `eloquent/model_date_cast.rs` 中单字段格式优先及存储隔离用例，参考旧版 `http/response.rs` 的创建状态适配。HTTP 状态与 JSON 继续沿用现有响应构造器，也对应 Goravel 将状态与数据交给响应接口的职责。 [Laravel 日期文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-serialization.md#customizing-the-date-format-per-attribute) · [Laravel Router](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/Router.php) · [Laravel 模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php#L3069) · [Laravel 路由测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Routing/RoutingRouteTest.php#L2166) · [Goravel 响应契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/http/response.go)
- `DateWithFormat` 和 `DateTimeWithFormat` 使用 Chrono 格式及应用连接时区，仅覆盖该字段的序列化。日期读取、存储和脏值判断继续用原有契约。路由通过 Rust 类型推导识别同步返回值、异步返回值及借用请求的模型或模型集合。刚创建的模型用 201，已有模型及集合用 200，模型缺失由异常管线渲染 404，其他失败走默认 500。统一业务封装仍可由路由显式选择。
- SQLite 与真实 MySQL 8.4.13 验证日期归零、微秒、时区偏移、空值、存储和等价写入。回环 TCP 验证 8 次模型请求，涵盖空集合、创建、并行读取、借用请求、缺失、脱敏 500 和失败后重试。三语路由示例逐个启动应用并各发送 2 次 HTTP 请求。Rust 1.94.0 隔离工作区 440 项测试、格式、严格 Clippy、`./artisan inspire`、VitePress 构建和 18 项浏览器检查通过。隔离副本保留已有验收基线，工作区中并行开发的 URL 生成器未合入。

## 枚举集合与值对象

- 对照固定 Laravel 的 `AsEnumCollection`、`AsEnumArrayObject`、值对象缓存和转换后脏值用例。提取旧版 `6d1a8192` 的 `eloquent/enum_collection_cast.rs` 中保留键、重新编号和读写分离，以及 `eloquent/class_cast_cache.rs` 中原始值快照与多列补丁的回归思路。Goravel 的连接级 ORM 职责继续沿用。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#value-object-casting) · [枚举集合](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Casts/AsEnumCollection.php) · [枚举数组对象](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Casts/AsEnumArrayObject.php) · [Laravel 模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php#L523) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- 两种枚举集合转换支持类型化读取、赋值和编辑。集合保留键与遍历顺序，数组对象赋值时重新编号。通用 JSON 赋值保存原始值，读取时验证枚举成员。Rust 的元素解析要求字符串或整数，未采用 PHP 的弱类型转换。
- `ValueObjectCast` 按实例缓存借用的对象。`editValueObject` 编辑独立副本，验证并应用多列补丁，随后由常规 `save` 持久化。Rust 通过显式编辑管理可变访问，补丁在编辑成功时更新模型属性。修改原始字段或转换配置会使缓存失效。`Castable` 参数工厂及 Laravel 的关闭对象缓存选项仍待核对。
- SQLite 与真实 MySQL 8.4.13 验证顺序、重新编号、空值、非法成员修复、对象缓存、多列更新、失败不写入以及模型实例隔离。Rust 1.94.0 隔离工作区 440 项测试、格式、全目标严格 Clippy、`./artisan inspire`、三语双库示例、VitePress 构建和 18 项浏览器检查通过。

## 加密类转换与 JSON 路径

- 对照固定 Laravel 的 `fillJsonAttribute`、加密数组对象及集合，以及加密路径赋值集成测试。提取旧版 `6d1a8192` 的 `eloquent/json_path.rs` 与 `encrypted_collection_cast.rs` 中保留同级字段、索引及字符串偏移、密钥轮换、形状检查和失败保护。继续沿用应用级加密器。 [Laravel 加密转换](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#encrypted-casting) · [Laravel 属性源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php) · [Laravel 集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelEncryptedCastingTest.php) · [Goravel Crypt Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/crypt/service_provider.go)
- `AsEncryptedArrayObject` 和 `AsEncryptedCollection` 提供加密结构化读取、显式集合编辑和存储。模型路径赋值先解密和构造完整文档，成功后只更新基础列。批量赋值尊重 `fillable`，受保护的基础列不能通过嵌套路径绕过。非空非法 JSON、畸形路径或解密失败均保留模型原状态。
- Rust 使用类型化集合及显式编辑，键顺序遵循现有 JSON 表示，路径深度遵循 Serde 解码能力。字符串偏移仅接受单字节替换，并约束填充增长。自定义加密集合工厂、静态 `encryptUsing`、PHP JSON 选项及键顺序的剩余差异仍待核对。
- SQLite 与真实 MySQL 8.4.13 验证保留同级字段、数组写入、空值、字符串偏移、加密类形状、旧密钥重写、新密钥读取和批量解密失败回滚。隔离工作区 440 项测试、格式、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建和 18 项浏览器检查通过。

## 转换工厂与条件可见性

- 对照固定 Laravel `Castable`、转换器参数、条件可见性与集合可见性，检查固定版自定义转换和模型测试。提取旧版 `6d1a8192` 的 `eloquent/class_cast.rs` 工厂与 `class_cast_cache.rs` 关闭对象缓存用例。转换仍发生在模型层。 [Laravel Castables](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#castables) · [Laravel 可见性源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HidesAttributes.php) · [Laravel 自定义转换测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/DatabaseCustomCastsTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- `Castable::castUsing` 构造双向或仅入站转换器，接入赋值、读取、脏值比较和序列化。Rust 的参数为有类型的静态声明，由模型或查询覆盖选择。工厂按操作解析，错误保持在调用处。值对象可关闭缓存，`getValueObject` 以 `Cow` 提供缓存借用或新构造的自有值。
- `makeVisibleIf` 与 `makeHiddenIf` 接受布尔值或当前模型谓词。集合可逐个应用显示、隐藏、替换名单和追加属性。SQLite 与真实 MySQL 8.4.13 验证参数隔离、持久化、仅入站读取、工厂失败后重试、无缓存读取、条件执行次数和集合可见性。
- 隔离工作区 Rust 1.94.0 440 项测试、格式、全目标严格 Clippy、`./artisan inspire`、六份三语示例双库执行、VitePress 构建和 24 项浏览器检查通过。

## 标准 JSON 集合转换

- 对照固定 Laravel `AsArrayObject`、`AsCollection`、映射和空值测试。提取旧版 `6d1a8192` 的 `eloquent/collection_cast.rs` 中有序键、可空存储、读取映射和原始 JSON 比较机制。沿用现有 Collection、Serde 与 Eloquent 写入管线，Goravel 的连接级查询职责保持一致。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Casts/AsCollection.php) · [Laravel 数组对象](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Casts/AsArrayObject.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/DatabaseCustomCastsTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- 两种转换默认把空赋值写成 JSON `null`，`nullable()` 写成 SQL NULL。类型化赋值保留集合键顺序，映射只作用于读取。Rust 用同一个有序 `Collection` 表达数组对象与集合，`using` 接收构造函数及可选映射，`of` 和 `getJsonCollectionAs` 使用 Serde。使用显式编辑写回，MySQL 原生 JSON 列的顺序遵循数据库返回值。
- SQLite 与真实 MySQL 8.4.13 验证存储空值区别、JSON 列与文本列、键顺序、嵌套赋值、映射失败和修复、编辑失败及重查。隔离工作区 Rust 1.94.0 440 项测试、格式、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建与 18 项浏览器检查通过。

## 二进制与向量转换

- 对照固定 Laravel 的 `AsBinary`、`BinaryCodec`、`AsVector` 与两类模型测试。核对旧版 `6d1a8192`，复用旧版 `attribute_cast.rs` 对 PHP 数字前缀的转换思路，沿用现有数据库 `Value::Bytes` 与绑定接口。Goravel 的连接级查询职责继续沿用。 [Laravel 二进制与向量文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#binary-casting) · [二进制源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Casts/AsBinary.php) · [向量源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Casts/AsVector.php) · [二进制测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentAsBinaryCastTest.php) · [向量测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentAsVectorCastTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- UUID 与 ULID 以 16 字节存储，读取和序列化返回规范字符串。自定义编解码器在字段转换中显式声明，避免跨应用共享可变注册表。沿用锁定的 `uuid 1.29.2`，加入 `ulid 1.2.1`，二者关闭默认特性。Rust 在赋值时拒绝错误的固定字节长度。
- 向量在已支持的 SQLite 和 MySQL 上使用 JSON 文本，并可读取 BLOB 中的小端 float32。浮点文本由 Serde 编码，原生 MariaDB `vec_fromtext` 和 PostgreSQL pgvector 的写入及查询等待相应驱动。模型脏值比较保留原始快照，允许覆盖旧的无效值。
- SQLite 与真实 MySQL 8.4.13 验证二进制字节、ULID 输入格式、自定义编解码器、向量和 BLOB 解码、非法赋值及修复重查。隔离工作区 Rust 1.94.0 440 项测试、格式、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建和 18 项浏览器检查通过。根工作区锁文件保留其他开发所需依赖，隔离验收仅加入本批依赖。

## 哈希转换

- 对照固定 Laravel 的属性写入逻辑及集成测试。复用旧版 `6d1a8192` 的 `database/eloquent/casts.rs` 和 `casts_tests.rs` 中明文与空字符串哈希、空值直通、已有哈希配置校验和低成本哈希原样保留的行为。Goravel 的 Hash Provider 同样从所属应用解析哈希服务。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#attribute-casting) · [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php) · [Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/EloquentModelHashedCastingTest.php) · [Goravel Hash Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/hash/service_provider.go)
- 模型和关系 `create`、模型 `update` 接通 `AttributeCast::Hashed`。Rust 通过 `fillAsync` 和 `setAttributeValueAsync` 执行明文赋值，沿用应用哈希服务的有界线程池。同步赋值可接收通过校验的哈希和空值。批量赋值全部成功后才写入模型状态。查询构建器的批量更新继续使用原始列值。关系创建的哈希分支使用装箱异步赋值，修复调试构建中的默认栈溢出。
- SQLite 与真实 MySQL 8.4.13 验证持久化重查、已有哈希保留、跨算法和超成本拒绝、批量赋值失败回滚、更新重试、关系创建、并行盐值和应用隔离。Rust 1.94.0 隔离工作区格式、442 项锁定离线测试、全目标严格 Clippy 和 `./artisan inspire` 通过。三语示例均在两库实际执行，VitePress 构建及根路径和子路径共 18 项浏览器检查通过。

## 链式字符串转换

- 对照固定 Laravel 的 `AsStringable`、`Stringable`、默认 Unicode 裁剪与模型集成测试。参考 Goravel 的拥有链式操作的字符串类型。复用旧版 `6d1a8192` 的 `support/stringable.rs` 字符集合、原样序列化和 `eloquent/stringable_cast.rs` 的文本赋值、重查与失败回归。 [Laravel 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md#stringable-casting) · [转换源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Casts/AsStringable.php) · [模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Database/DatabaseCustomCastsTest.php) · [Goravel String](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/support/str/str.go)
- `AsStringable::cast()` 支持文本和常用标量赋值。SQL NULL 返回空的类型化结果，模型 JSON 投影为字符串或 null。Rust 通过已有的 `getValueObject` 和 `setValueObject` 显式读写类型化字符串，`trim` 只修改调用者取得的字符串，赋值时保留原文。拒绝无法转换的输入并保留模型原值。此处的 Support 类型提供本转换所需的字符串契约，完整链式方法清单随 07-01 验收。
- SQLite 与真实 MySQL 8.4.13 验证空值、空字符串、Unicode 裁剪、内部字符保留、标量赋值、无效赋值后原值、脏值与持久化重查。Rust 1.94.0 隔离工作区格式、445 项锁定离线测试、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建及 18 项浏览器检查通过。

## Fluent 属性转换

- 对照固定 Laravel `AsFluent`、`Fluent` 与 Support 测试。参考 Goravel 将 ORM 与连接管理交给所属应用。复用旧版 `6d1a8192` 的 `support/fluent.rs`、`support/data.rs` 和 `eloquent/fluent_cast.rs` 中点路径、显式空值、数组键投影和类型化回写。更新通配写入以匹配 Laravel 的 `data_set`。 [Laravel 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Casts/AsFluent.php) · [Fluent 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportFluentTest.php) · [Goravel ORM](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/orm/orm.go)
- Rust 使用 `getValueObject` 和 `editValueObject` 读写类型化对象。无效路径或超深数据返回类型化错误，失败保留原值。数据库 SQL NULL 与 JSON null 分别处理。JSON 标量可编码写入，转换为属性对象时要求数组或对象。使用现有 `serde_json` 的 `preserve_order`，文本列保留键顺序，数据库原生 JSON 列遵循驱动返回顺序。HTTP 限流回归检查响应字段和值，MySQL 关系测试把大型测试 Future 移到堆上以适配默认调试栈。Support 的完整数据辅助接口留在 07-01。
- SQLite 与真实 MySQL 8.4.13 验证有序存储、显式空值、通配读写、字面键、深度错误回滚、类型化及 JSON 路径回写、标量读失败后的修复和持久化重查。Rust 1.94.0 隔离工作区格式、447 项锁定离线测试、全目标严格 Clippy、`./artisan inspire`、三语示例双库执行、VitePress 构建和 18 项浏览器检查通过。
- 本批依赖工作区尚未提交的 `ValueObjectCast` 和模型对象 API。按现有 HEAD 仅提交 Fluent 文件会缺少这些依赖，待核定可独立构建的 Eloquent 提交范围。

05-03 继续核对 `AsUri` 的 URI 类型依赖，之后复核自定义集合子类和剩余加密扩展。

## URI 转换与交付复验

- 对照固定 Laravel 的 [AsUri 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Casts/AsUri.php)，复用旧版 `6d1a8192` 的 `support/uri/parse.rs` 和 `eloquent/uri_cast.rs` 的延迟解析与字符串投影。文本赋值保留原文，读取和 JSON 投影解析并规范化，类型化赋值存储 URI 的字符串。覆盖绝对 URI、相对引用、显式空查询和片段、无效值读取与重新赋值、SQL NULL。
- 基于 05-02 本地提交 `41f6cf0c` 独立构建。Rust 1.94.0 格式、447 项锁定离线工作区测试、全目标严格 Clippy、`artisan inspire` 通过。Eloquent 测试使用 SQLite 和真实 MySQL。回环 HTTP 验证 URI 投影与隐藏字段、无效 URI 的 500、模型缺失的 404 和后续请求恢复。三语文档构建通过，通用浏览器 12 项通过。
- Rust 的集合工厂返回有序 `Collection`，自定义模型对象通过类型化转换表达。应用级加密服务提供密钥轮换和隔离。Laravel 的 PHP 集合子类机制与静态全局 `encryptUsing` 仍有接口差异。URI 修改器与查询构造接口随 Support 单元扩展。模型转换、持久化和序列化完成本单元验收。
