# Blade 模板

Blade 模板使用 `.blade.html` 后缀。普通 `.html` 视图以 UTF-8 HTML 输出。

## 显示数据

在 `resources/views/greeting.blade.html` 中显示[路由](./views.md)传入的 `name`。

```html
<h1>Hello, {{ $name }}</h1>
```

双花括号会转义 HTML 字符。下面分别演示可信 HTML、客户端表达式和 Blade 注释。用 `@verbatim` 和 `@endverbatim` 保留整段文本。

```text
{!! $trusted !!}
@{{ client }}
{{-- comment --}}
```

## Blade 指令

用 `@if`、`@elseif`、`@else`、`@endif`、`@unless` 和 `@endunless` 判断视图数据。用 `@foreach ($items as $item)` 和 `@endforeach` 遍历。表达式支持标量字面量、变量及 `$user->name` 等对象属性。OpenRust 使用自己的模板编译器与 Rust 编译产物格式。 可用 `@isset` 和 `@empty` 检查数据，`@forelse` 可以通过 `@empty` 处理空集合。

用 `@includeIf('partials.notice')` 跳过缺失的视图。用 `@includeWhen($showNotice, 'partials.notice')` 或 `@includeUnless($hideNotice, 'partials.notice')` 按条件渲染。子视图继承当前视图数据，包括循环变量。条件成立并选中视图时，`@includeWhen` 和 `@includeUnless` 要求该视图存在。

## 组件

将匿名组件 `notice` 放在 `resources/views/components/notice.blade.html`。带引号的属性是字符串，属性名前加 `:` 可以传入视图表达式。默认插槽用 `$slot` 读取，命名插槽用对应名称读取。插槽内容是已渲染的 HTML，插槽内输出的数据仍按普通规则转义。

```html
<aside><strong>{{ $heading }}</strong>{{ $slot }}</aside>
```

## 构建布局

在 `resources/views/layouts/app.blade.html` 中用 `@yield` 放置区段。子视图可以继承布局并填充区段，用 `@parent` 引用继承的区段内容。

```html
<main>{{ $site }}: @yield('content')</main>
```

将子模板放在 `resources/views/dashboard.blade.html`，从路由返回 `view("dashboard")`。

```html
@extends('layouts.app')
@section('content')
<x-notice>
    <x-slot:heading>{{ $title }}</x-slot:heading>
    <p>Ready</p>
</x-notice>
@endsection
```
