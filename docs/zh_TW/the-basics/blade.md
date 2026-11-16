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

## Blade 指令

使用 `@if`、`@elseif`、`@else`、`@endif`、`@unless` 和 `@endunless` 判斷視圖資料。使用 `@foreach ($items as $item)` 和 `@endforeach` 走訪資料。運算式支援純量常值、變數及 `$user->name` 等物件屬性。OpenRust 使用自己的範本編譯器與 Rust 編譯成品格式。 可用 `@isset` 和 `@empty` 檢查資料，`@forelse` 可以透過 `@empty` 處理空集合。

以 `@includeIf('partials.notice')` 略過不存在的檢視。以 `@includeWhen($showNotice, 'partials.notice')` 或 `@includeUnless($hideNotice, 'partials.notice')` 依條件轉譯。子檢視繼承目前的檢視資料，包括迴圈變數。條件成立並選中檢視時，`@includeWhen` 和 `@includeUnless` 要求該檢視存在。

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

### 元件屬性與屬性袋 {#component-attributes}

在元件範本開頭用 `@props` 宣告屬性及預設值。呼叫端可以用 `display-name` 傳值給 `displayName`。宣告以外的 HTML 屬性可透過 `$attributes` 讀取。

```html
@props(['displayName' => 'Notice', 'variant',])
<aside {{ $attributes->merge(['class' => 'panel', 'role' => 'status']) }}>
    {{ $displayName }}{{ $slot }}
</aside>
```

使用 `{{ $attributes }}` 輸出屬性袋。`merge` 提供預設值，一般屬性以呼叫端的值為準，`class` 和 `style` 合併預設值與呼叫端的文字。HTML 屬性值會跳脫，null 和 false 會省略。

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

用 `@once` 在同一呈現樹中只輸出一次內容。`@pushOnce` 和 `@prependOnce` 也接受可跨範本共用的選用識別碼。
