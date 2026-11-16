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

## 查询作用域

实现 `Scope<Model>` 可复用查询约束。用 `withScope` 应用它，或在模型的 `globalScopes` 注册全局作用域。使用 `withoutGlobalScope` 移除指定作用域。以下示例使用上面的 `Flight` 模型，数据表需包含已声明字段和时间戳。

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
