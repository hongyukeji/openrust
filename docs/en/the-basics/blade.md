# Blade Templates

Save Blade templates as `.blade.html`. A plain `.html` view is served as UTF-8 HTML.

## Displaying Data

In `resources/views/greeting.blade.html`, display the `name` passed by the [route](./views.md).

```html
<h1>Hello, {{ $name }}</h1>
```

Double braces escape HTML entities. The following examples emit trusted HTML, preserve a client-side expression, and add a Blade comment. Use `@verbatim` and `@endverbatim` to preserve a larger block.

```text
{!! $trusted !!}
@{{ client }}
{{-- comment --}}
```

## Blade Directives

Use `@if`, `@elseif`, `@else`, `@endif`, `@unless`, and `@endunless` with view values. Use `@foreach ($items as $item)` and `@endforeach` to iterate. Expressions support scalar literals, variables and object properties such as `$user->name`. Templates use OpenRust's compiler and a Rust artifact format. Use `@isset` and `@empty` to test data. A `@forelse` loop can render an `@empty` branch.

## Components

Store an anonymous component such as `notice` in `resources/views/components/notice.blade.html`. A quoted attribute is a string. Prefix its name with `:` to pass a view expression. Render the default slot with `$slot` and access a named slot by its name. Slot content is already rendered HTML. Data echoed inside the slot is escaped normally.

```html
<aside><strong>{{ $heading }}</strong>{{ $slot }}</aside>
```

## Building Layouts

In `resources/views/layouts/app.blade.html`, use `@yield` to place a section. The child view can extend the layout and fill the section. Use `@parent` to include content from the inherited section.

```html
<main>{{ $site }}: @yield('content')</main>
```

Place the child template in `resources/views/dashboard.blade.html` and return `view("dashboard")` from the route.

```html
@extends('layouts.app')
@section('content')
<x-notice>
    <x-slot:heading>{{ $title }}</x-slot:heading>
    <p>Ready</p>
</x-notice>
@endsection
```
