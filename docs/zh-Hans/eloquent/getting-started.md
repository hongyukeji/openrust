# Eloquent 入门

## 生成模型类

运行 `./artisan make:model Flight` 生成 `app/models/flight.rs`。在 `app/models/mod.rs` 声明 `mod flight;`，并在 `app/mod.rs` 导出 `models` 模块后重新构建。使用 `--force` 可覆盖生成的文件。Rust 模型在 `ModelMetadata` 中声明数据表，每个实例持有一个 `ModelState`。

```rust
use openrust::database::eloquent::{Model, ModelMetadata, ModelState};

pub struct Flight(ModelState);

impl Model for Flight {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("flights")
            .fillable(&["name", "active"])
            .softDeletes("deleted_at");
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
```

`getFillable()` 和 `getGuarded()` 返回模型声明的批量赋值名单。`Model::isUnguarded()` 查询当前绕过保护的作用域；`Model::unguarded(future)` 在完成、失败或取消时恢复保护，不影响其他任务。`getDeletedAtColumn()` 返回配置的软删除列 `Option<&str>`，未启用软删除时为 `None`。

## 模型约定

默认主键为自增的 `id`，并维护 `created_at` 和 `updated_at`。在 `ModelMetadata` 中通过 `primaryKey`、`incrementing`、`timestamps` 或 `connection` 配置模型。使用 `fillable` 声明可批量赋值的字段。

### UUID 与 ULID 主键 {#uuid-and-ulid-keys}

使用 `UniqueId::Uuid` 生成 UUIDv7，或使用 `UniqueId::Ulid` 生成小写 ULID。默认生成主键，并将其作为非自增字符串。使用 `uniqueIds` 指定生成列。插入时，Eloquent 在 `Saving` 之后、`Creating` 之前填充空值，保留已提供的非空值。复制时排除这些列，新记录插入时重新生成标识符。这些列的路由模型绑定遇到格式错误会返回 404。

```rust
use openrust::database::eloquent::{ModelMetadata, UniqueId};

static TICKET: ModelMetadata = ModelMetadata::new("tickets")
    .uniqueId(UniqueId::Uuid)
    .uniqueIds(&["id", "reference"]);
```

### 写入后刷新属性 {#refreshing-attributes-after-writes}

通过 `ModelMetadata::refreshes` 声明由数据库管理的列。插入或实际更新后，Eloquent 从写连接读取这些列，再调用 `Created` 或 `Updated` 监听器。显式事务在同一事务内回读。复制模型时排除这些列，由数据库为新行计算值。

```rust
use openrust::database::eloquent::ModelMetadata;

static ARTICLE: ModelMetadata = ModelMetadata::new("articles")
    .fillable(&["title"])
    .refreshes(&["title_length"]);
```

## 查询模型

在应用请求或命令中使用 `Flight::query()?` 获取模型查询构造器，使用 `Flight::all().await?` 获取应用作用域后的模型集合，或用 `Flight::find(id).await?` 按主键查找。模型不存在时，`findOrFail` 返回 `DatabaseError::ModelNotFound`。可用 `queryOn` 指定数据库连接。

### 模型字段引用（扩展） {#model-field-references}

从已有数据库表自动生成模型字段引用，不需要逐个声明字段。先执行应用迁移，再选择新建或刷新模型：

```sh
./artisan make:model User --fields
# 已有模型只刷新字段，保留模型业务代码
./artisan model:fields User
# 自定义表、连接和嵌套模型目录
./artisan model:fields Admin/User --table=accounts --database=pgsql
```

新模型仍需按 `make:model` 提示在父 `mod.rs` 中声明模块。重新构建后，`model:fields` 从空模型的 `getTable()` 和 `resolveConnectionName()` 读取表名与连接，无需反复传参。单模型的 `--table`、`--database` 可覆盖本次读取。刷新会将标准 `metadata()` 静态初始化中的表名字面量替换为 `User::TABLE`，其余模型配置保持原文；显式 `--table` 会更新生成的表名常量，重新构建后标准模型使用该表。自定义 `getTable()`、动态表达式和非标准初始化不会被重写。创建时 `--table` 与 `--database` 会同时保存到新模型元数据，后者须搭配 `--fields`；已发布的自定义模型模板使用 `{{connection}}` 占位符接收连接配置。

首次刷新尚未登记的旧模型时，优先读取标准 `metadata()` 初始化中的表名字面量，否则按模型名转下划线并取复数推断；连接使用应用默认值，自定义连接及动态表名需显式指定。随后重新构建，后续同步便可读取模型配置。修改模型表名、连接或模块声明后，应先重新构建命令程序；源码表名字面量与已编译模型不一致时，命令拒绝隐式同步，需重新构建或显式指定 `--table`。自定义动态配置仍以已编译模型为准。

```sh
# 明确选择多个模型
./artisan model:fields User Admin/User
# 同步全部已编译且启用自动字段同步的应用模型
./artisan model:fields --all
# CI 检查：结果相同返回 0，过期返回 1，不修改模型源码
./artisan model:fields --all --check
```

`--all` 只包含 `app/models/` 约定目录中已编译并携带自动生成区域的模型，不扫描全部数据库表，也不自动发现任意 trait 实现或包模型。首次接入、旧生成区域升级或新增模型后，先逐个刷新、声明模块并构建。批量同步使用各模型自己的配置，不接受共用的表或连接覆盖。命令先预检全部模型，再逐个原子替换；预检失败不写入模型，写入阶段的磁盘错误可能留下部分已完成结果，修复后可重复执行。`--check` 不创建源码锁或源码临时文件；缺表、无效连接、损坏的生成分区也返回非零并说明错误，应用启动自身的日志或缓存仍沿用原行为。

生成后即可使用 IDE 补全：

```rust
let mut query = User::query()?;
query.r#where(User::NAME, "=", "张三");
let users = query.orderBy(User::ID, "asc").get().await?;
```

数据库当前列结构是生成集合的来源，`fillable` 和 `casts` 仍各自控制批量赋值与属性转换。命令将“模型表与字段”分区放在模型文件的导入之后、类型定义之前，使用配置式说明和简洁声明 `openrust::model_fields! { User { TABLE = "users"; NAME = "name"; } }`。刷新时自动将旧标记区域迁移到新布局，保留模型业务代码；将生成源码一起纳入版本管理，普通构建、请求和部署无需连接数据库。不要手改生成区域；迁移新增、删除或重命名列后，重新运行同步命令、修复受影响引用并构建。没有同步时，编译器不会自动感知外部数据库变更。无需该能力的动态模型和字符串查询保持原用法；普通 `make:model` 不访问数据库，也不生成表名和字段常量。

生成顺序固定为表名 `TABLE` 在前，其后保留数据库实际列顺序，不按名称排序，也不单独提前主键。`User::TABLE` 可传给已有查询入口，模型元数据通过 `ModelMetadata::new(User::TABLE)` 复用表名；表别名仍由现有查询接口处理。

列名转换为大写下划线常量，保留实际数据库列名，例如 `display-name` → `DISPLAY_NAME`、`1st` → `COLUMN_1ST`。`TABLE` 保留给表名，数据库的 `table` 列生成 `COLUMN_TABLE`；它与真实 `column_table` 列重名时预检失败。转换后重名、非 ASCII 列名或无法形成标识时，命令失败并保留原文件；缺表、缺连接或生成分区损坏也不会覆盖模型。特殊列可继续用字符串，或使用高级 `model_fields!` 宏做显式映射。由旧手写声明迁移时先移除重复声明；自动生成不会替你删除手写常量。生成区外已有手写 `TABLE` 时也会拒绝写入，请先消除重复定义。

常量实际类型是 `&'static str`。不存在或同步后已删除的常量引用会编译失败；**不检查查询模型归属、查询值类型或构建时的数据库结构**。NULL、数值、日期、JSON、自定义 cast 和表达式沿用现有规则，没有新增值类型保证。

列名不自动附带表名。选择字段使用 `select([User::NAME])`；更新使用 `[(User::NAME.into(), value)].into()` 作为属性映射；关系键直接传常量。联表或表别名可用 `format!("u.{}", User::NAME)`，再传给已有查询接口。列名仍由语法器作为标识符处理，值继续参数绑定；不要把常量拼入 `whereRaw` 等原始 SQL。动态字段和原始表达式保持原入口。

数据库列注释会生成为 Rust 文档注释 `///`（需要转义控制字符时使用 `#[doc = "…"]`），IDE 悬停即可查看字段描述。MySQL、PostgreSQL 通过现有结构接口提供列注释；SQLite 不保存原生列注释。缺失或空白注释时不生成描述。修改列注释后也需刷新，`--check` 会检测这类变化。描述直接取自数据库，不根据字段名猜测，也不在生成时翻译。

`TABLE` 上方直接显示数据库表注释，不添加前缀；没有注释或注释为空白时使用当前语言的默认说明，例如“表名”。表注释变更后也需同步，`--check` 会检测这类变化。

分区标题、同步说明及表名标签跟随 `app.locale`（`APP_LOCALE`），内置英文、简体中文与繁体中文；可通过现有 Artisan 的 `console.model_fields` 翻译键定制。切换语言后重新同步模型；数据库表注释和字段注释保持原文。

### 刷新模型 {#refreshing-models}

`fresh()` 回读独立实例，`freshWithPaths(&["posts.comments"])` 还能预加载已登记路径。`refresh()` 更新当前实例，并重载已加载的登记关系。两者都按原主键从写连接回读，跳过全局作用域并包含软删除记录。未保存的模型调用 `fresh()` 得到 `None`，调用 `refresh()` 保持原状。数据库记录已移除时，`refresh()` 返回 `ModelNotFound`。需要重载的关系须在 `namedRelations()` 登记，中间表 Pivot 投影保留。关系重载失败时，当前属性保留刚读到的值，原始快照保持此前的值。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn reload<M: Model>(model: &mut M) -> DatabaseResult<Option<M>> {
    let copy = model.fresh().await?;
    model.refresh().await?;
    Ok(copy)
}
```

用 `refreshForUpdate(&mut transaction)` 在事务内刷新模型。MySQL 行锁由调用方的事务持有，提交或回滚时释放。事务须从模型所属连接开启。同连接的已加载关系及其读取事件可以看到事务内尚未提交的改动。跨连接关系使用各自的连接。自定义关系可实现 `matchResultsInTransaction` 参与事务内加载。SQLite 在事务内读取，遵循 SQLite 的事务锁规则。

```rust
use openrust::database::{DatabaseResult, Transaction, eloquent::Model};

pub async fn reload_locked<M: Model>(
    model: &mut M,
    transaction: &mut Transaction,
) -> DatabaseResult<()> {
    model.refreshForUpdate(transaction).await?;
    Ok(())
}
```

### 游标 {#cursors}

调用 `cursor()` 可在轮询时逐行水合模型。模型作用域、属性转换与 `Retrieved` 事件仍然生效。需要预加载关系时，请使用分批查询。读取期间游标占用一个数据库连接。如果读取事件监听器还要查询，需要另一个可用连接。

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{DatabaseResult, eloquent::Model};

async fn first_model<M: Model>() -> DatabaseResult<Option<M>> {
    let mut models = M::query()?.cursor();
    models.try_next().await
}
```

### 分批读取结果 {#chunking-results}

Eloquent 查询的 `chunk(count, callback)` 将每页作为模型集合交给异步回调。没有指定排序时按主键排序。回调执行前，每批都会触发模型读取事件、预加载关系并调用模型集合工厂。参见[查询构建器的分批规则](../database/queries.md#chunking-results)。

Eloquent 也支持 `chunkById` 和 `chunkByIdDesc`。传入 `None` 时以模型主键分批。后续查询的边界值取自 SQL 投影，请保留分批列或其投影别名。每批仍会执行 Eloquent 的读取事件及预加载。

Eloquent 的 `each` 将模型及其当前批次中的 `ArrayKey` 传给回调，保留自定义集合键。`eachById` 将批次偏移加到整数集合键上。集合键无法表示为有符号整数时返回错误。

Eloquent 的惰性方法在返回模型前按批水合并预加载。每批在消费模型前释放数据库连接。`chunkMap` 限制每次读取的输入模型数量，返回的集合包含全部映射结果。

## 插入和更新模型

`create` 按批量赋值规则保存新模型。`setAttribute` 设置单个属性，`save` 用原主键写入已变更的属性。写入成功后同步模型快照。构造器的 `update` 和 `delete` 返回受影响行数。

### 更新计数器 {#updating-counters}

使用 `increment`、`decrement`、`incrementEach` 或 `decrementEach` 原子更新数值属性。多列方法接收列名与步长的映射，返回受影响的行数。已持久化的模型会触发 `Updating` 和 `Updated`，并回读写入的属性。未持久化的实例将操作应用于带作用域的查询。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model}};

pub async fn record_visit<M: Model>(model: &mut M) -> DatabaseResult<u64> {
    model.incrementEach(
        Attributes::from([("visits".into(), 1.into()), ("views".into(), 2.into())]),
        Attributes::new(),
    ).await
}
```

实例更新时使用带 `Quietly` 后缀的方法可抑制模型事件。Eloquent Builder 提供同名的四个运算方法，按查询作用域批量更新。批量更新会写入模型的更新时间，调用方可以显式提供时间戳。批量更新不会派发逐模型事件。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model}};

pub async fn count_visit_quietly<M: Model>(model: &mut M) -> DatabaseResult<u64> {
    model.incrementQuietly("visits", 1_i64, Attributes::new()).await
}
```

在 Eloquent 查询上使用 `fillAndInsert(rows)`，批量插入前会应用模型默认值、查询预设属性、修改器和转换。构造器补齐缺失的唯一标识和时间戳，保留显式时间戳，并绕过批量赋值保护和模型持久化事件。`fillAndInsertOrIgnore(rows)` 使用驱动的忽略插入语义，`fillAndInsertGetId(row)` 返回单条记录由数据库生成的 ID。`fillForInsert(rows)` 仅返回准备好的属性。

## 删除模型

使用 `softDeletes("deleted_at")` 启用软删除。模型查询默认排除软删除记录，可用 `withTrashed` 或 `onlyTrashed` 查询它们，用 `restore` 清除删除时间，用 `forceDelete` 永久删除。

## 复制模型 {#replicating-models}

调用 `replicate(&[]).await?` 创建尚未保存的副本。框架会排除主键、已配置的时间戳和内部中间表键。可传入其他要排除的列名。副本保留已加载的关系，也可以单独移除关系。副本沿用源模型的数据库连接并触发 `Replicating` 生命周期事件。使用 `replicateQuietly` 跳过该事件，调用 `save` 插入副本。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

async fn copy_without_secret<M: Model>(source: &M) -> DatabaseResult<M> {
    source.replicate(&["secret"]).await
}
```

### 严格属性策略

在应用启动时通过 `Flight::shouldBeStrict(&app, true)?` 为模型开启严格策略，也可分别调用 `preventLazyLoading(&app, enabled)`、`preventSilentlyDiscardingAttributes(&app, enabled)`、`preventAccessingMissingAttributes(&app, enabled)`。默认关闭，配置属于当前应用和模型类型；脱离应用的模型需要通过 `newInstanceOn` 关联应用连接，或在当前应用上下文中使用策略。

严格批量赋值遇到被丢弃字段时返回 `DatabaseError::MassAssignment`，此前已赋值的字段保留在实例上。`forceFill`、`forceFillAsync` 和 `unguarded` 继续绕过保护。可覆盖 `handleDiscardedAttributeViolationUsing` 处理被丢弃字段，返回成功表示允许丢弃。

使用 `getAttributeValue` 读取需要检查的属性。开启防护后，已检索模型缺少属性时返回 `DatabaseError::MissingAttribute`；显式读取到的 SQL NULL、访问器、未保存模型和刚创建模型仍按其正常行为处理。可覆盖 `handleMissingAttributeViolationUsing` 为未知属性提供值；原生或枚举转换器在处理钩子成功后继续按空值转换。`getAttribute` 和 `getRawOriginal` 保留底层 `Option` 读取。懒加载策略在水合多模型结果时保存到实例，由隐式关系读取检查；切换默认值影响后续水合，模型显式覆盖的 `preventsLazyLoading` 仍然生效。

### 临时禁用关联时间戳更新

`Owner::withoutTouching(work).await` 在 Future 执行期间抑制该模型类型作为关联目标的时间戳更新。导入 `std::any::TypeId` 后，可用 `Owner::withoutTouchingOn(&[TypeId::of::<Owner>(), TypeId::of::<Team>()], work).await` 指定多个类型。`isIgnoringTouch()` 查询是否禁用关联时间戳更新；没有更新时间戳的模型也返回 true。保存子模型仍写入其自身属性并派发模型事件，关联 Saved 事件和后续关联遍历保持正常行为。嵌套作用域在成功、错误、恐慌或取消后恢复此前设置，不影响其他任务。

## 查询作用域

实现 `Scope<Model>` 可复用查询约束。用 `withScope` 应用它，或在模型的 `globalScopes` 注册全局作用域。使用 `withoutGlobalScope` 移除指定作用域。以下示例使用上面的 `Flight` 模型，数据表需包含已声明字段和时间戳。

`withGlobalScope(name, scope)` 为当前查询注册或替换作用域，接收 `Scope<M>` 实现或返回 `DatabaseResult<()>` 的 `Send + Sync + 'static` 闭包。作用域的 `extend` 钩子在注册时执行，`apply` 在应用作用域时执行。`withoutGlobalScopes()` 移除当前已注册的作用域，之后重新注册的仍会生效；`withoutGlobalScopesExcept(&["tenant"])` 仅保留指定名称。`removedScopes()` 保留移除调用的顺序和重复名称。注册表属于当前查询，不会改变其他查询或模型类型。

软删除使用同一注册表。移除其他作用域时需要保留软删除过滤，可导入 `eloquent::SoftDeletingScope` 并将 `std::any::type_name::<SoftDeletingScope>()` 加入保留名单。`onlyTrashed()` 或 `withoutTrashed()` 显式添加的条件不会因移除全部已注册作用域而消失。

`applyScopes()` 消费模型查询构造器并返回已应用作用域的构造器，重复调用会再次应用当前注册项。`getQuery()` 消费模型查询构造器并返回当前底层查询，不自动应用尚未执行的作用域；需要这些约束时调用 `applyScopes()?.getQuery()`。`setQuery(query)` 替换底层查询及其旧条件，保留模型作用域与预加载配置。`getEagerLoads()` 借用类型化的 `EagerLoads<M>`，可用 `names()` 读取关系名称；`setEagerLoads(loads)` 替换配置，也可设置为空。覆盖 `Model::newEloquentBuilder(query)` 并用 `Builder::fromQuery(query)` 构造，可定制公开模型查询使用的构造器。工厂返回框架的类型化 Builder，额外业务方法使用 Rust trait 或包装类型扩展，不使用运行期类名。

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Attributes, Builder, Scope},
};

struct Active;

impl Scope<Flight> for Active {
    fn apply(&self, query: &mut Builder<'_, Flight>) -> DatabaseResult<()> {
        query.r#where("active", "=", true);
        Ok(())
    }
}

pub async fn save_flight(connection: &Connection) -> DatabaseResult<()> {
    let mut flight = Flight::queryOn(connection)
        .create(Attributes::from([
            ("name".into(), "Flight 42".into()),
            ("active".into(), true.into()),
        ]))
        .await?;
    flight.setAttribute("name", "Flight 42A");
    flight.save().await?;

    let mut flights = Flight::queryOn(connection);
    flights.withScope(&Active)?;
    assert_eq!(flights.get().await?.len(), 1);

    flight.delete().await?;
    assert!(flight.trashed());
    flight.restore().await?;
    Ok(())
}
```

## 在事务中写入模型 {#model-transactions}

将同一可变事务传给 `saveInTransaction`、`updateInTransaction`、`deleteInTransaction`、`forceDeleteInTransaction` 或 `restoreInTransaction`。由调用方提交或回滚。已有来源连接的模型必须使用该连接的事务。监听器的 SQL 若要加入同一事务，应实现 `ModelEventListener::handleInTransaction` 并通过收到的事务执行。现有基于连接的回调继续接收池化连接。回滚撤销数据库写入。已反映本次写入的模型实例应丢弃或重新查询。

```rust
use openrust::database::{Connection, DatabaseResult, eloquent::Model};

async fn save_in_transaction<M: Model>(
    connection: &Connection,
    model: &mut M,
) -> DatabaseResult<bool> {
    let mut transaction = connection.beginTransaction().await?;
    match model.saveInTransaction(&mut transaction).await {
        Ok(true) => {
            transaction.commit().await?;
            Ok(true)
        }
        Ok(false) => {
            transaction.rollBack(None).await?;
            Ok(false)
        }
        Err(error) => {
            transaction.rollBack(None).await?;
            Err(error)
        }
    }
}
```

## 模型事件 {#model-events}

应用启动时用 `Model::registerModelEvent` 注册类型化监听器。监听器可以在写入前修改模型，也可返回 `ModelEventResponse::Cancel` 取消写入。成功保存会触发 `Saved`，包括属性没有变化的情况。查询构造器的批量更新和删除不派发实例事件。使用 `saveQuietly`、`deleteQuietly`、`forceDeleteQuietly`、`restoreQuietly` 或 `Model::withoutEvents` 在当前异步调用链内抑制模型事件。写入后监听器的错误会返回给调用方，此时数据库写入已执行。

```rust
use openrust::{
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{
            Model,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
        },
    },
    foundation::Application,
};

struct RequireTitle;

impl<M: Model> ModelEventListener<M> for RequireTitle {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            Ok(match model.getAttribute("title") {
                Some(openrust::database::Value::Text(title)) if !title.trim().is_empty() => {
                    ModelEventResponse::Continue
                }
                _ => ModelEventResponse::Cancel,
            })
        })
    }
}

fn register<M: Model>(app: &Application) -> DatabaseResult<()> {
    M::registerModelEvent(app, ModelEvent::Saving, RequireTitle)
}
```

### 映射模型事件 {#mapping-model-events}

实现 `dispatchesEvents`，根据模型当前的生命周期阶段构造事件，通过[应用派发器](../digging-deeper/events.md)注册相应监听器。写前监听器可返回 `ListenerResponse::Value(Box::new(false))` 取消写入。没有响应时回退到普通模型监听器及观察者，已处理的响应优先。事务写入会将当前事务传给派发器。提交后监听器收到生命周期事件触发时构造的自有载荷。

```rust
use std::sync::Arc;
use openrust::{
    contracts::events::Event,
    database::{Value, eloquent::{Model, ModelMetadata, ModelState, concerns::ModelEvent}},
    events::NamedEvent,
};

struct Flight(ModelState);

impl Model for Flight {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("flights").fillable(&["name"]);
        &META
    }

    fn from_state(state: ModelState) -> Self { Self(state) }
    fn state(&self) -> &ModelState { &self.0 }
    fn state_mut(&mut self) -> &mut ModelState { &mut self.0 }

    #[allow(non_snake_case)]
    fn dispatchesEvents(&self, event: ModelEvent) -> Option<Arc<dyn Event>> {
        let (ModelEvent::Saved, Some(Value::Text(name))) = (event, self.getAttribute("name")) else {
            return None;
        };
        Some(Arc::new(NamedEvent::new("flight.saved", name.clone())))
    }
}
```

### 观察者 {#observers}

运行 `./artisan make:observer FlightObserver --model=Flight`，在 `app/observers/flight_observer.rs` 生成五个写入后生命周期方法。声明模块，并在服务提供者启动时注册观察者。省略 `--model` 会生成普通结构体，使用 `--force` 可覆盖已有文件。通过 `./artisan stub:publish` 发布并自定义 `observer.rs.stub` 和 `observer.plain.rs.stub`。

用 `Observer<M>` 集中实现模型生命周期方法，在服务提供者启动时调用 `M::observe` 注册。通过 `EVENTS` 指定要注册的生命周期方法。方法接收模型及本次操作的数据库连接，写前方法可以返回 `ModelEventResponse::Cancel` 取消写入。注册表归当前应用所有。

```rust
use openrust::{
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{Model, Observer, concerns::{ModelEvent, ModelEventResponse}},
    },
    foundation::Application,
};

struct NormalizeTitle;

impl<M: Model> Observer<M> for NormalizeTitle {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::Saving];

    fn saving<'a>(
        &'a self,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if let Some(openrust::database::Value::Text(title)) = model.getAttribute("title") {
                let title = title.trim().to_owned();
                model.setAttribute("title", title);
            }
            Ok(ModelEventResponse::Continue)
        })
    }
}

fn register<M: Model>(app: &Application) -> DatabaseResult<()> {
    M::observe(app, NormalizeTitle)
}
```

#### 观察者与数据库事务 {#observers-and-database-transactions}

实现 `ShouldHandleEventsAfterCommit` 并通过 `Model::observeAfterCommit` 注册。使用显式模型事务时，六个写前回调仍即时执行，可取消写入。其他回调等待最外层提交成功。事务或保存点回滚时丢弃相应回调。无事务时即时执行。Rust 要求模型可克隆，并在事件触发时保存模型状态。延后回调失败时，提交返回 `DatabaseError::AfterCommit`，数据库写入已经提交。

```rust
use openrust::{
    contracts::events::ShouldHandleEventsAfterCommit,
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{Model, Observer, concerns::{ModelEvent, ModelEventResponse}},
    },
    foundation::Application,
};

struct ArticleCommitted;

impl ShouldHandleEventsAfterCommit for ArticleCommitted {}

impl<M: Model> Observer<M> for ArticleCommitted {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::Created];

    fn created<'a>(
        &'a self,
        _model: &'a mut M,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            connection.insert("INSERT INTO article_audit (action) VALUES (?)", &["created".into()]).await?;
            Ok(ModelEventResponse::Continue)
        })
    }
}

fn register<M: Model + Clone>(app: &Application) -> DatabaseResult<()> {
    M::observeAfterCommit(app, ArticleCommitted)
}
```

## 主键与单值查询 {#keys-and-single-values}

`whereKey`、`whereKeyNot`、`orWhereKey`、`orWhereKeyNot` 接收单值、`Vec` 或数组。整数主键列表使用整数条件，字符串主键列表使用绑定值；正向空列表不匹配任何行，反向空列表匹配全部行。主键列带模型表限定，OR 条件仍受全局作用域约束。字符串主键模型应覆盖 `getKeyType()` 返回 `ModelKeyType::String`，此时单值主键按字符串转换。传模型实例时，显式传入其 `getKey()` 的值。动态条件可以用 `eloquent::Key` 表示单值或列表。

```rust
let mut query = User::query()?;
query.whereKey([1, 2, 3]).orWhereKeyNot(4);
let users = query.get().await?;
```

`value(column)` 返回首个模型的属性，无结果返回 `Value::Null`。`valueOrFail(column)` 在无结果时返回模型不存在错误。`sole()` 和 `soleValue(column)` 要求恰好一行：零行返回模型不存在错误，多行返回 `DatabaseError::MultipleModelsFound`。这些接口经过模型水合，保留访问器、类型转换、检索事件与全局作用域。已选列不会被覆盖；只选择 `id` 后查询其他属性，不会自动补查。实际行的字段为 SQL NULL 时，`valueOrFail` 仍返回 `Value::Null`。

## 保存与删除变体 {#write-variants}

`saveOrFail()` 在模型连接上开启事务。`updateOrFail(attributes)` 先填充已存在的模型，再在事务中保存；`deleteOrFail()` 在事务中删除已存在的模型。事件取消返回 `false`，SQL 或监听器错误回滚写入并向上传播；异步任务取消也经现有机制释放并回滚事务。数据库回滚不会恢复模型内存中的属性或存在标记，应丢弃或重新读取受影响实例。需要共用外层事务时，将同一个可变事务传给 `saveInTransaction`、`updateInTransaction` 或 `deleteInTransaction`；自定义事务保存逻辑也在这些类型化钩子中实现。

`updateQuietly(attributes)` 先填充已存在的模型，再抑制实例事件保存。两个更新变体对不存在的模型在填充前返回 `false`；`deleteOrFail` 此时也不会开启事务。

`saveOrIgnore(None)` 插入新模型并忽略唯一冲突，`Some(&["email"])` 指定冲突列。发生冲突返回 `false`，不标记模型已存在，也不触发 `Created` / `Saved`；插入成功会取得生成主键并发出正常创建事件。对已存在模型调用会报错。该方法复用驱动的插入忽略并返回能力：PostgreSQL、SQLite 支持；MySQL、SQL Server 对非空插入明确返回不支持错误。该方法不会自动开启事务，也不会吞掉监听器错误。关闭时间戳且没有属性的空模型可能返回 `true` 而不插入行。

`original` 只在保存后事件与关联触碰成功后，同步最终模型属性；保存后监听器失败时保留待保存变化，监听器成功修改的属性也纳入同步。
