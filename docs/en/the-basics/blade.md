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

Use `@includeIf('partials.notice')` to skip an absent view. Use `@includeWhen($showNotice, 'partials.notice')` or `@includeUnless($hideNotice, 'partials.notice')` to render conditionally. These directives inherit the current view data, including loop variables. A selected view must exist for `@includeWhen` and `@includeUnless`.

### The Loop Variable {#the-loop-variable}

Both `@foreach` and `@forelse` accept `$key => $item`. Arrays expose zero-based integer keys. Objects expose string keys in the order of the view data map.

```html
@foreach ($groups as $key => $items)
    <h2>{{ $key }}</h2>
    @foreach ($items as $item)
        <p>{{ $loop->parent->iteration }}.{{ $loop->iteration }} {{ $item }}</p>
    @endforeach
@endforeach
```

`$loop` exposes `iteration` (one-based), `index` (zero-based), `remaining`, `count`, `first`, `last`, `odd`, `even`, `depth`, and `parent`. The outermost loop has depth 1 and a null parent. An included view can read the outer loop and use it as the parent of a nested loop.

## Components

Store an anonymous component such as `notice` in `resources/views/components/notice.blade.html`. A quoted attribute is a string. Prefix its name with `:` to pass a view expression. Render the default slot with `$slot` and access a named slot by its name. Slot content is already rendered HTML. Data echoed inside the slot is escaped normally.

```html
<aside><strong>{{ $heading }}</strong>{{ $slot }}</aside>
```

### Component Props and Attributes {#component-attributes}

Put `@props` at the start of the component template. The list can contain prop names and default values. A caller can pass a camel-case prop such as `displayName` using `display-name`. Attributes outside the declaration are available through `$attributes`.

```html
@props(['displayName' => 'Notice', 'variant',])
<aside {{ $attributes->merge(['class' => 'panel', 'role' => 'status']) }}>
    {{ $displayName }}{{ $slot }}
</aside>
```

Render `{{ $attributes }}` to emit the bag. `merge` adds defaults and preserves caller values for ordinary attributes. `class` and `style` combine their default and caller text. HTML attribute values are escaped. Null and false are omitted.

### Dynamic Components {#dynamic-components}

Use `<x-dynamic-component>` when the view data selects a registered Rust component or an anonymous component. The `component` attribute selects the target. Other attributes and slots are passed to that component.

```html
<x-dynamic-component :component="$componentName" :title="$label">
    <x-slot:header>{{ $app }}</x-slot:header>
    <x-dynamic-component component="badge" text="<&"/>
</x-dynamic-component>
```

`<x-accordion>` searches `components/accordion.blade.html`, `components/accordion/index.blade.html`, then `components/accordion/accordion.blade.html`. A registered Rust component takes precedence.

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

## Forms {#forms}

In a session-enabled web route, `@csrf` emits the current session token as a hidden `_token` field. `@error` renders the first message for the field and makes it available as `$message` inside the block. Pass a second argument to read a named error bag.

```html
<form method="post">
    @csrf
    @error('name')
        <span>{{ $message }}</span>
    @enderror
</form>
```

`@method('PUT')` emits a hidden `_method` field. OpenRust routing currently matches the HTTP method sent by the client.

## Stacks {#stacks}

Use `@push` or `@prepend` to collect markup during rendering. A layout can display it with `@stack`. `@hasStack` can conditionally render the surrounding markup.

```html
@pushOnce('scripts', 'dashboard')
<script src="/dashboard.js" defer></script>
@endPushOnce

@hasStack('scripts')
    @stack('scripts')
@endif
```

## PHP and Rust Templates {#php-and-rust-templates}

Prepare values that require PHP function calls or `@php` blocks in a Rust handler, view creator, or composer, then pass them to the view. For PHP class components and dynamic class discovery, register the corresponding Rust `Component` factory with `Factory::component`. Anonymous components resolve through the view finder.

Use `@once` for content that should render only once per view tree. `@pushOnce` and `@prependOnce` also accept an optional ID shared across templates.
