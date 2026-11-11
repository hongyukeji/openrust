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
