# Blade 範本

Blade 範本使用 `.blade.html` 副檔名。一般 `.html` 視圖以 UTF-8 HTML 輸出。

## 顯示資料

在 `resources/views/greeting.blade.html` 中顯示[路由](./views.md)傳入的 `name`。

```html
<h1>Hello, {{ $name }}</h1>
```

雙大括號會跳脫 HTML 字元。以下分別示範可信 HTML、用戶端運算式和 Blade 註解。使用 `@verbatim` 和 `@endverbatim` 保留整段文字。

```text
{!! $trusted !!}
@{{ client }}
{{-- comment --}}
```

### HTML 實體編碼 {#html-entity-encoding}

Blade 預設會對跳脫輸出中的現有 HTML 實體再次編碼。若要保留有效的 HTML 4.01 實體，可在應用程式服務提供者啟動時設定 Blade 編譯器。呼叫 `withDoubleEncoding()` 可恢復預設行為。

```rust
use openrust::view::compilers::BladeCompiler;

fn configure_encoding(compiler: &BladeCompiler) {
    compiler.withoutDoubleEncoding();
}
```

### 輸出 JavaScript 資料 {#rendering-json}

在指令碼中用 `Js::from` 或 `@js` 將視圖值序列化為 JavaScript 運算式。呈現引擎會跳脫 HTML 敏感字元、引號和行分隔符。

```html
<script>
    const settings = {{ Js::from($settings) }};
</script>
```

## Blade 指令

使用 `@if`、`@elseif`、`@else`、`@endif`、`@unless` 和 `@endunless` 判斷視圖資料。使用 `@foreach ($items as $item)` 和 `@endforeach` 走訪資料。運算式支援純量常值、變數及 `$user->name` 等物件屬性。OpenRust 使用自己的範本編譯器與 Rust 編譯成品格式。 可用 `@isset` 和 `@empty` 檢查資料，`@forelse` 可以透過 `@empty` 處理空集合。

以 `@includeIf('partials.notice')` 略過不存在的檢視。以 `@includeWhen($showNotice, 'partials.notice')` 或 `@includeUnless($hideNotice, 'partials.notice')` 依條件轉譯。子檢視繼承目前的檢視資料，包括迴圈變數。條件成立並選中檢視時，`@includeWhen` 和 `@includeUnless` 要求該檢視存在。

### 驗證指令 {#authentication-directives}

在 HTTP 視圖中用 `@auth` 和 `@guest` 檢查已設定的守衛。傳入名稱可以指定守衛。`@elseauth` 和 `@elseguest` 在同一區塊中增加分支。

```html
@auth('staff')
    <p>Staff</p>
@elseguest
    <p>Sign in</p>
@endauth
```

### 環境、工作階段與上下文 {#environment-session-context}

用 `@production` 和 `@env` 比對應用程式環境。`@env` 接受單一名稱或清單，支援萬用字元。`@session` 和 `@context` 在各自區塊內提供 `$value`。工作階段支援點分隔鍵，`@context` 讀取公開的請求上下文。

```html
@env(['staging-*', 'local'])
    <p>Preview</p>
@endenv
@session('notice')
    <p>{{ $value }}</p>
@endsession
@context('request.id')
    <p>{{ $value }}</p>
@endcontext
```

### 包含子視圖 {#including-subviews}

傳入映射可以覆蓋繼承的視圖資料。`@includeFirst` 呈現清單中第一個存在的視圖。`@includeIsolated` 只接收明確傳入的資料和視圖工廠的共用資料。

```html
@includeFirst(['custom.notice', 'partials.notice'], ['label' => $title])
@includeIsolated('partials.notice', ['label' => 'Opening soon'])
```

`@each` 為每個項目呈現一個子視圖。子視圖可讀取 `$key`、指定名稱的項目和視圖共用資料。第四個參數可指定空集合視圖，也可用 `raw|` 前綴傳入可信的常值。

```html
@each('partials.job', $jobs, 'job', 'partials.empty')
```

### 運算式與條件屬性 {#conditional-attributes}

條件和輸出運算式可使用括號、`!`、`&&`、`||`、`===`、`!==` 和 `??`。嚴格比較保留純量型別。PHP 的物件身分和寬鬆型別轉換超出視圖資料模型的範圍。

`@class` 和 `@style` 接受常值清單。獨立項目一律輸出，`文字 => 條件` 在條件為真時輸出。`@checked`、`@selected`、`@disabled`、`@readonly` 和 `@required` 在條件為真時輸出對應的 HTML 屬性。

```html
<input @checked($ready && $count === 2) @class(['field', 'active' => $ready])>
{{ $label ?? 'Pending' }}
```

`@for` 使用有號整數計數器，支援 `++`、`--`、`+=` 和 `-=`。`@while` 在條件為真時重複。`@break` 和 `@continue` 可接收條件或常值巢狀層數。每次呈現中，計數迴圈與 `@while` 共用 100,000 次迭代上限。計數器的作用域為目前的迴圈。

```html
@for ($index = 0; $index < 3; $index++)
    @continue($index === 1)
    {{ $index }}
@endfor
```

使用 `@switch`、`@case`、`@default` 與 `@endswitch` 依視圖值分支。選中的分支會依序執行後續分支，直到 `@break` 或整個區塊結束。數值字串可比對數值分支。`@bool` 輸出 `true` 或 `false` 文字。

```html
@switch($state)
    @case('ready')
        Ready
        @break
    @default
        Pending
@endswitch
```

### 迴圈變數 {#the-loop-variable}

`@foreach` 和 `@forelse` 都接受 `$key => $item`。陣列的鍵是從零開始的整數。物件依視圖資料映射的順序提供字串鍵。

```html
@foreach ($groups as $key => $items)
    <h2>{{ $key }}</h2>
    @foreach ($items as $item)
        <p>{{ $loop->parent->iteration }}.{{ $loop->iteration }} {{ $item }}</p>
    @endforeach
@endforeach
```

`$loop` 提供 `iteration`（從 1 開始）、`index`（從 0 開始）、`remaining`、`count`、`first`、`last`、`odd`、`even`、`depth` 和 `parent`。最外層迴圈的深度為 1，父迴圈為 null。子視圖可以讀取外層迴圈，並將它用作巢狀迴圈的父迴圈。

## 元件

將匿名元件 `notice` 放在 `resources/views/components/notice.blade.html`。帶引號的屬性是字串，屬性名稱前加上 `:` 可以傳入視圖運算式。預設插槽用 `$slot` 讀取，具名插槽用對應名稱讀取。插槽內容是已呈現的 HTML，插槽內輸出的資料仍依一般規則跳脫。

```html
<aside><strong>{{ $heading }}</strong>{{ $slot }}</aside>
```

`:$title` 是 `:title="$title"` 的簡寫。`::` 前綴保留 `:class` 等用戶端繫結。已註冊視圖命名空間的元件可用 `<x-demo::panel>` 存取。

### 插槽屬性 {#slot-attributes}

可以用 `<x-slot:footer-actions>` 或 `<x-slot name="footerActions">` 宣告帶屬性的插槽。透過 `$footerActions->attributes` 讀取屬性袋，可呼叫 `merge`、`class`、`style`、`only` 和 `except`。

```html
<x-demo::panel :$title ::class="active">
    <x-slot:footer-actions data-role="actions">Save</x-slot>
</x-demo::panel>
```

```html
<footer {{ $footerActions->attributes->class(['toolbar']) }}>
    {{ $footerActions }}
</footer>
```

### 元件屬性與屬性袋 {#component-attributes}

在元件範本開頭用 `@props` 宣告屬性及預設值。呼叫端可以用 `display-name` 傳值給 `displayName`。宣告以外的 HTML 屬性可透過 `$attributes` 讀取。

```html
@props(['displayName' => 'Notice', 'variant',])
<aside {{ $attributes->merge(['class' => 'panel', 'role' => 'status']) }}>
    {{ $displayName }}{{ $slot }}
</aside>
```

使用 `{{ $attributes }}` 輸出屬性袋。`merge` 提供預設值，一般屬性以呼叫端的值為準，`class` 和 `style` 合併預設值與呼叫端的文字。HTML 屬性值會跳脫，null 和 false 會省略。

屬性袋方法從左到右執行。`whereStartsWith` 和別名 `thatStartWith` 保留名稱相符的屬性，`whereDoesntStartWith` 排除相符項目。可以傳入單一前綴或前綴清單。插槽屬性袋也支援這些方法。

```html
<aside {{ $attributes->whereStartsWith(['data-', 'aria-'])->class(['panel']) }}>
    {{ $slot }}
</aside>
```

#### 作用域插槽 {#scoped-slots}

已註冊的 Rust 元件可透過 `Component::data` 提供視圖值。插槽透過 `$component` 讀取這些值，元件範本透過一般變數讀取同一份資料。元件方法的結果在 Rust 中計算後傳入。

```html
<x-scoped-panel>
    <x-slot:heading>{{ $component->caption }}</x-slot:heading>
</x-scoped-panel>
```

### 讀取父元件資料 {#component-parent-data}

在子元件範本開頭使用 `@aware`，可與 `@props` 一起宣告。最近一層元件明確傳入的值優先。父元件透過 `@props` 宣告的預設值只用於該父元件。

```html
@aware(['color' => 'neutral'])
<span>{{ $color }}</span>
```

### 匿名元件目錄 {#anonymous-component-paths}

在應用程式組裝時註冊額外的匿名元件目錄。透過命名空間指向視圖根目錄下的目錄，或透過路徑指定檔案系統目錄。`view:cache` 也會編譯已註冊路徑中的範本。

```rust
use std::path::Path;
use openrust::view::{Factory, ViewResult};

fn configure_components(factory: &Factory, external: &Path) -> ViewResult<()> {
    factory.anonymousComponentNamespace("widgets", Some("widget"))?;
    factory.anonymousComponentPath(external, Some("external"))
}
```

```html
<x-widget::banner/>
<x-external::notice/>
```

### 動態元件 {#dynamic-components}

視圖資料可以透過 `<x-dynamic-component>` 選擇已註冊的 Rust 元件或匿名元件。`component` 屬性指定目標，其餘屬性和插槽傳給目標元件。

```html
<x-dynamic-component :component="$componentName" :title="$label">
    <x-slot:header>{{ $app }}</x-slot:header>
    <x-dynamic-component component="badge" text="<&"/>
</x-dynamic-component>
```

`<x-accordion>` 依序尋找 `components/accordion.blade.html`、`components/accordion/index.blade.html` 和 `components/accordion/accordion.blade.html`。已註冊的 Rust 元件優先。

## 建立版面

在 `resources/views/layouts/app.blade.html` 中用 `@yield` 放置區段。子視圖可以繼承版面並填入區段，用 `@parent` 引用繼承的區段內容。

```html
<main>{{ $site }}: @yield('content')</main>
```

將子範本放在 `resources/views/dashboard.blade.html`，從路由回傳 `view("dashboard")`。

```html
@extends('layouts.app')
@section('content')
<x-notice>
    <x-slot:heading>{{ $title }}</x-slot:heading>
    <p>Ready</p>
</x-notice>
@endsection
```

用 `@hasSection('content')` 或 `@sectionMissing('content')` 判斷版面區段是否有內容，並用 `@endif` 結束。

用 `@extendsFirst` 選擇第一個可用的版面。兩個繼承指令都可明確傳入視圖資料。用 `@append` 附加區塊或用 `@overwrite` 覆寫區塊。`@stop` 與 `@endsection` 都可結束區塊。

```html
@extendsFirst(['themes.card', 'layouts.app'], ['site' => 'OpenRust'])
```

## 表單 {#forms}

在啟用 Session 的 Web 路由中，`@csrf` 將目前的工作階段權杖輸出為隱藏的 `_token` 欄位。`@error` 在欄位發生錯誤時呈現區塊，並在區塊內用 `$message` 提供第一則訊息。第二個參數可指定具名錯誤袋。

```html
<form method="post">
    @csrf
    @method('PUT')
    @error('name')
        <span>{{ $message }}</span>
    @enderror
</form>
```

瀏覽器 POST 表單可以送到 PUT、PATCH 或 DELETE 路由。在 `@csrf` 後寫入 `@method('PUT')`，即可輸出隱藏的 `_method` 欄位。路由依覆寫後的方法比對，Web 中介軟體仍會驗證 CSRF。處理常式可透過 `request.getRealMethod()` 讀取原始請求方法。

POST 請求優先使用 `X-HTTP-Method-Override` 標頭，其次使用表單欄位，最後讀取查詢參數。

## 堆疊 {#stacks}

用 `@push` 或 `@prepend` 收集呈現片段，再由版面透過 `@stack` 輸出。`@hasStack` 可以根據堆疊是否有內容決定是否輸出外層標記。

```html
@pushOnce('scripts', 'dashboard')
<script src="/dashboard.js" defer></script>
@endPushOnce

@hasStack('scripts')
    @stack('scripts')
@endif
```

`@pushIf` 接收條件和堆疊名稱。可用 `@elsePushIf` 新增另一個條件分支。

```html
@pushIf($showAssets, 'scripts')
    <script src="/assets/extra.js" defer></script>
@endPushIf
```

## 呈現 Blade 片段 {#rendering-blade-fragments}

用 `@fragment` 和 `@endfragment` 標記範本片段。從路由回傳 `view("dashboard").fragment("summary")` 就能傳送該片段。

```html
@fragment('summary')
<h2>{{ $title }}</h2>
@endfragment
```

`fragments(["summary", "details"])` 依參數順序回傳指定片段。`allFragments()` 回傳全部已擷取片段。可以用 `fragmentIf(condition, "summary")` 或 `fragmentsIf(condition, names)` 在條件為假時傳送完整範本。OpenRust 每次回應只呈現一次視圖，選擇多個片段時也是如此。

## 服務注入 {#service-injection}

透過 `Factory::registerViewService` 註冊具型別的服務，並回傳範本所需的資料。在範本開頭用 `@inject` 將已註冊的服務別名繫結到變數。

```rust
use openrust::view::{Factory, ViewResult};

struct ReportSummary {
    total: u64,
}

fn expose_summary(factory: &Factory) -> ViewResult<()> {
    factory.registerViewService("report.summary", |summary: &ReportSummary| {
        Ok(summary.total.into())
    })
}
```

```html
@inject('total', 'report.summary')
<p>{{ $total }}</p>
```

## 呈現內嵌 Blade 範本 {#rendering-inline-blade-templates}

`Factory::renderInline(source, &data)` 在記憶體中呈現受信任的範本文字。子視圖由同一個工廠解析。

## 擴充 Blade {#extending-blade}

在應用程式組裝時呼叫 `BladeCompiler::directive` 註冊自訂指令。回呼接收求值後的視圖資料。`DirectiveOutput::Text` 會跳脫，`DirectiveOutput::Html` 作為可信 HTML 輸出。

```rust
use openrust::view::{ViewResult, compilers::{BladeCompiler, DirectiveOutput}};

fn configure_blade(compiler: &BladeCompiler) -> ViewResult<()> {
    compiler.directive("formatName", |arguments| {
        let name = arguments.first().and_then(|value| value.as_str()).unwrap_or_default();
        Ok(DirectiveOutput::Text(name.to_owned()))
    })
}
```

```html
<b>@formatName($name)</b>
```

### 自訂條件 {#custom-conditions}

使用 `BladeCompiler::r#if` 註冊述詞。述詞接收求值後的視圖資料，也可以在 Rust 中用 `check` 呼叫同一述詞。

```rust
use openrust::view::{ViewResult, compilers::BladeCompiler};

fn configure_conditions(compiler: &BladeCompiler, current_zone: String) -> ViewResult<()> {
    compiler.r#if("zone", move |args| {
        Ok(args.first().and_then(|value| value.as_str()) == Some(current_zone.as_str()))
    })
}
```

```html
@zone('public')
    Public
@elsezone('staff')
    Staff
@else
    Restricted
@endzone
```

## PHP 與 Rust 範本 {#php-and-rust-templates}

PHP 函式呼叫或 `@php` 程式區塊需要的值，可以在 Rust 處理器、視圖建立器或組合器中準備，再傳入視圖。PHP 類別元件及其動態類別探索對應 `Factory::component` 明確註冊的 Rust `Component` 工廠。匿名元件由視圖尋找器解析。

用 `@once` 在同一呈現樹中只輸出一次內容。`@pushOnce` 和 `@prependOnce` 也接受可跨範本共用的選用識別碼。
