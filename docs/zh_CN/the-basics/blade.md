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

### 循环变量 {#the-loop-variable}

`@foreach` 和 `@forelse` 均接受 `$key => $item`。数组的键是从零开始的整数。对象按视图数据映射的顺序提供字符串键。

```html
@foreach ($groups as $key => $items)
    <h2>{{ $key }}</h2>
    @foreach ($items as $item)
        <p>{{ $loop->parent->iteration }}.{{ $loop->iteration }} {{ $item }}</p>
    @endforeach
@endforeach
```

`$loop` 提供 `iteration`（从 1 开始）、`index`（从 0 开始）、`remaining`、`count`、`first`、`last`、`odd`、`even`、`depth` 和 `parent`。最外层循环的深度为 1，父循环为 null。子视图可以读取外层循环，并将其用作嵌套循环的父循环。

## 组件

将匿名组件 `notice` 放在 `resources/views/components/notice.blade.html`。带引号的属性是字符串，属性名前加 `:` 可以传入视图表达式。默认插槽用 `$slot` 读取，命名插槽用对应名称读取。插槽内容是已渲染的 HTML，插槽内输出的数据仍按普通规则转义。

```html
<aside><strong>{{ $heading }}</strong>{{ $slot }}</aside>
```

### 组件属性与属性包 {#component-attributes}

在组件模板开头用 `@props` 声明属性和默认值。调用方可以用 `display-name` 向 `displayName` 传值。声明之外的 HTML 属性可以通过 `$attributes` 读取。

```html
@props(['displayName' => 'Notice', 'variant',])
<aside {{ $attributes->merge(['class' => 'panel', 'role' => 'status']) }}>
    {{ $displayName }}{{ $slot }}
</aside>
```

使用 `{{ $attributes }}` 输出属性包。`merge` 添加默认值，普通属性以调用方的值为准，`class` 和 `style` 合并默认值与调用方的文本。HTML 属性值会转义，null 和 false 会省略。

### 动态组件 {#dynamic-components}

视图数据可以通过 `<x-dynamic-component>` 选择一个已注册的 Rust 组件或匿名组件。`component` 属性指定目标，其余属性和插槽传给目标组件。

```html
<x-dynamic-component :component="$componentName" :title="$label">
    <x-slot:header>{{ $app }}</x-slot:header>
    <x-dynamic-component component="badge" text="<&"/>
</x-dynamic-component>
```

`<x-accordion>` 依次查找 `components/accordion.blade.html`、`components/accordion/index.blade.html` 和 `components/accordion/accordion.blade.html`。已注册的 Rust 组件优先。

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

## 表单 {#forms}

在启用 Session 的 Web 路由中，`@csrf` 将当前会话令牌输出为隐藏的 `_token` 字段。`@error` 在有字段错误时渲染区块，并在块内用 `$message` 提供首条消息。第二个参数可指定命名错误包。

```html
<form method="post">
    @csrf
    @error('name')
        <span>{{ $message }}</span>
    @enderror
</form>
```

`@method('PUT')` 输出隐藏的 `_method` 字段。OpenRust 路由目前根据客户端发送的 HTTP 方法匹配。

## 栈 {#stacks}

用 `@push` 或 `@prepend` 收集渲染片段，再由布局通过 `@stack` 输出。`@hasStack` 可以根据栈是否有内容决定是否输出外围标记。

```html
@pushOnce('scripts', 'dashboard')
<script src="/dashboard.js" defer></script>
@endPushOnce

@hasStack('scripts')
    @stack('scripts')
@endif
```

## 渲染 Blade 片段 {#rendering-blade-fragments}

用 `@fragment` 和 `@endfragment` 标记模板片段。从路由返回 `view("dashboard").fragment("summary")` 即可发送该片段。

```html
@fragment('summary')
<h2>{{ $title }}</h2>
@endfragment
```

`fragments(["summary", "details"])` 按参数顺序返回指定片段。`allFragments()` 返回全部已捕获片段。可以用 `fragmentIf(condition, "summary")` 或 `fragmentsIf(condition, names)` 在条件为假时发送完整模板。OpenRust 每次响应只渲染一次视图，选择多个片段时也一样。

## PHP 与 Rust 模板 {#php-and-rust-templates}

PHP 函数调用或 `@php` 代码块需要的值，可以在 Rust 处理器、视图创建器或组合器中准备，再传给视图。PHP 类组件及其动态类发现对应 `Factory::component` 显式注册的 Rust `Component` 工厂。匿名组件从视图查找器解析。

用 `@once` 在同一渲染树中只输出一次内容。`@pushOnce` 和 `@prependOnce` 也接受可跨模板共用的可选标识。
