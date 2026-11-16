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

### Including Subviews {#including-subviews}

Pass a map to override inherited data. `@includeFirst` renders the first existing view in the list. `@includeIsolated` receives its explicit values and the view factory's shared data.

```html
@includeFirst(['custom.notice', 'partials.notice'], ['label' => $title])
@includeIsolated('partials.notice', ['label' => 'Opening soon'])
```

Render one partial per entry with `@each`. The partial gets `$key`, the named item, and shared view data. A fourth argument can name an empty-state view. Use the `raw|` prefix for a trusted literal fallback.

```html
@each('partials.job', $jobs, 'job', 'partials.empty')
```

### Expressions and Conditional Attributes {#conditional-attributes}

Use parentheses, `!`, `&&`, `||`, `===`, `!==`, and `??` in conditions and echoes. Strict comparison preserves scalar types. PHP object identity and loose coercion have no corresponding operation in the view data model.

Use `@class` and `@style` with literal lists. Plain entries always render, and entries written as `text => condition` render when the condition is true. `@checked`, `@selected`, `@disabled`, `@readonly`, and `@required` emit their HTML attribute when true.

```html
<input @checked($ready && $count === 2) @class(['field', 'active' => $ready])>
{{ $label ?? 'Pending' }}
```

Use `@for` with a signed integer counter and `++`, `--`, `+=`, or `-=`. `@while` repeats while its expression is truthy. `@break` and `@continue` accept a condition or a literal nesting level. Counted and while loops share a 100,000-iteration limit per render. The counter is scoped to its loop.

```html
@for ($index = 0; $index < 3; $index++)
    @continue($index === 1)
    {{ $index }}
@endfor
```

Use `@switch`, `@case`, `@default`, and `@endswitch` to branch on a view value. A case continues into the next case until `@break` or the end of the switch. Numeric strings can match numeric cases. `@bool` writes the text `true` or `false`.

```html
@switch($state)
    @case('ready')
        Ready
        @break
    @default
        Pending
@endswitch
```

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

Use `:$title` as shorthand for `:title="$title"`. A `::` prefix preserves a client-side binding such as `:class`. Components under a registered view namespace use tags such as `<x-demo::panel>`.

### Slot Attributes {#slot-attributes}

Use `<x-slot:footer-actions>` or `<x-slot name="footerActions">` for a slot with attributes. Read an attribute bag through `$footerActions->attributes`. The bag supports `merge`, `class`, `style`, `only`, and `except`.

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

### Component Props and Attributes {#component-attributes}

Put `@props` at the start of the component template. The list can contain prop names and default values. A caller can pass a camel-case prop such as `displayName` using `display-name`. Attributes outside the declaration are available through `$attributes`.

```html
@props(['displayName' => 'Notice', 'variant',])
<aside {{ $attributes->merge(['class' => 'panel', 'role' => 'status']) }}>
    {{ $displayName }}{{ $slot }}
</aside>
```

Render `{{ $attributes }}` to emit the bag. `merge` adds defaults and preserves caller values for ordinary attributes. `class` and `style` combine their default and caller text. HTML attribute values are escaped. Null and false are omitted.

### Accessing Parent Component Data {#component-parent-data}

Declare `@aware` at the start of the child template, alongside `@props`. The nearest component's explicitly passed value wins. A default defined only by the parent's `@props` stays local to that parent.

```html
@aware(['color' => 'neutral'])
<span>{{ $color }}</span>
```

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

Use `@hasSection('content')` or `@sectionMissing('content')`, closed with `@endif`, to test whether a layout section has content.

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

## Rendering Blade Fragments {#rendering-blade-fragments}

Mark part of a template with `@fragment` and `@endfragment`. Return `view("dashboard").fragment("summary")` from the route to send that fragment.

```html
@fragment('summary')
<h2>{{ $title }}</h2>
@endfragment
```

`fragments(["summary", "details"])` returns the named fragments in the given order. `allFragments()` returns every captured fragment. Use `fragmentIf(condition, "summary")` or `fragmentsIf(condition, names)` to keep the full template when the condition is false. OpenRust evaluates the view once per response, including when selecting multiple fragments.

## Rendering Inline Blade Templates {#rendering-inline-blade-templates}

`Factory::renderInline(source, &data)` renders trusted template text in memory. Nested views resolve through that factory.

## Extending Blade {#extending-blade}

Call `BladeCompiler::directive` during application setup to register a custom directive. A callback receives evaluated view values. `DirectiveOutput::Text` is escaped, and `DirectiveOutput::Html` is emitted as trusted HTML.

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

### Custom Conditions {#custom-conditions}

Register a predicate with `BladeCompiler::r#if`. The condition receives evaluated view values. `check` invokes the same predicate from Rust.

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

## PHP and Rust Templates {#php-and-rust-templates}

Prepare values that require PHP function calls or `@php` blocks in a Rust handler, view creator, or composer, then pass them to the view. For PHP class components and dynamic class discovery, register the corresponding Rust `Component` factory with `Factory::component`. Anonymous components resolve through the view finder.

Use `@once` for content that should render only once per view tree. `@pushOnce` and `@prependOnce` also accept an optional ID shared across templates.
