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

### Arithmetic Expressions {#arithmetic-expressions}

Use `+`, `-`, `*`, `/`, `%` and `**` in echoes, conditions, array entries, includes and component attributes. Multiplication precedes addition; powers associate to the right and precede unary minus.

```html
{{ 1 + 2 * 3 }}
{{ (1 + 2) * 3 }}
{{ 2 ** 3 ** 2 }}
@if ($price * $quantity >= 100)Bulk order @endif
```

The first three echoes output `7`, `9` and `512`. Signed 64-bit integer overflow promotes to a double. Numeric strings, booleans and null follow PHP arithmetic conversion; non-numeric operands, division by zero and undefined variables fail rendering. `??`, `@isset` and `@empty` permit absent values; `&&`, `||` and `??` short-circuit. A failed render uses the application's exception response policy.

Fractional floating-point modulo operands truncate to integers, and a zero base with a negative exponent produces infinity. These operations emit deprecations rather than failing rendering, following Laravel's exception bootstrap. Configure `logging.deprecations.channel` to retain them; logging failures do not interrupt rendering. Testing skips these warnings unless `LOG_DEPRECATIONS_WHILE_TESTING` is truthy. Optional trace logging records a native Rust backtrace and error snapshot, and source locations identify the native evaluator rather than a PHP compiled template.

Direct scalar echoes, arithmetic, truth tests and comparisons preserve `INF`, `NAN` and signed zero. Native [view data](./views.md#native-view-values) also preserves these values through arrays, includes, components and custom directives. Templates do not execute arbitrary PHP or call unregistered functions. The compiler invalidates older expression artifacts automatically.

### HTML Entity Encoding {#html-entity-encoding}

Escaped Blade echoes double-encode existing HTML entities by default. To preserve valid HTML 4.01 entities in an application, configure its Blade compiler during provider startup. Use `withDoubleEncoding()` to restore the default.

```rust
use openrust::view::compilers::BladeCompiler;

fn configure_encoding(compiler: &BladeCompiler) {
    compiler.withoutDoubleEncoding();
}
```

### Rendering JavaScript Data {#rendering-json}

Use `Js::from` or `@js` inside a script to serialize a view value as a JavaScript expression. The renderer escapes HTML-sensitive characters, quotes, and line separators.

```html
<script>
    const settings = {{ Js::from($settings) }};
</script>
```

`Js::from` and `@js` reject `INF`, `-INF` and `NAN`, including nested values. The render error follows the application exception policy; a failed render does not poison subsequent renders. JSON output omits an integral float’s `.0`, and preserves negative zero as `-0`.

## Blade Directives

Use `@if`, `@elseif`, `@else`, `@endif`, `@unless`, and `@endunless` with view values. Use `@foreach ($items as $item)` and `@endforeach` to iterate. Expressions support scalar literals, variables and object properties such as `$user->name`. Templates use OpenRust's compiler and a Rust artifact format. Use `@isset` and `@empty` to test data. A `@forelse` loop can render an `@empty` branch.

Use `@includeIf('partials.notice')` to skip an absent view. Use `@includeWhen($showNotice, 'partials.notice')` or `@includeUnless($hideNotice, 'partials.notice')` to render conditionally. These directives inherit the current view data, including loop variables. A selected view must exist for `@includeWhen` and `@includeUnless`.

### Directive Boundaries and Whitespace {#directive-whitespace}

Built-in directive names follow Laravel's case-insensitive compiler methods; registered custom names remain case-sensitive. Separate a directive from a preceding ASCII letter, digit or underscore. For example, `word@if(...)` is text, while `<b>word</b>@if(...)` starts a condition. HTML comments and punctuation can provide a boundary; Blade comments cannot separate a preceding ASCII word from a directive. Non-ASCII text does not count as an ASCII word byte.

```html
@if (true)
    <b>Visible</b>
@endif
```

Put a directive's opening parenthesis on the same line, allowing spaces or tabs between its name and `(`. Unknown directives, including unknown `name::member` names, stay as text. `@@if` emits `@if`; bare `@@` stays unchanged. Use `@verbatim` for a larger literal block.

Blade reproduces PHP closing-tag behavior: a generated operation consumes one immediately following LF, CRLF or CR. A space or tab before that newline preserves it. Escaped and raw echo expressions preserve their following newline. Each compiled view removes leading PHP whitespace bytes (space, tab, LF, CR, vertical tab and NUL); form feed and non-breaking space remain. Plain `.html` views preserve their original content. Older compiled artifacts are invalidated automatically.

### Comments and Verbatim Compilation {#source-preparation}

Blade protects complete `@verbatim` blocks before removing <code v-pre>{{-- ... --}}</code> comments from the entire source. Comments disappear even inside quoted expressions, directive names and arguments, or component attributes. They can join tokens; they do not act as whitespace.

```html
@i{{-- note --}}f (true)
    {{ 'a{{-- note --}}b' }}
    {{ 1 {{-- note --}} + 2 }}
@endif
```

The echoes produce `ab` and `3`. Inside an original verbatim block, comments, echoes and component tags remain literal. Removing a comment cannot create a newly protected verbatim block. Leading whitespace after `@verbatim` still follows the closing-tag newline rules above. Incomplete comments, echoes and verbatim markers stay as text when no complete delimiter pair exists. Verbatim content inside quoted expressions is restored before evaluating and escaping the expression. Compiled artifacts are regenerated automatically; no PHP runtime is required.

### Authentication Directives {#authentication-directives}

Use `@auth` and `@guest` in HTTP views to check the configured guard. Pass a guard name to select a different one. `@elseauth` and `@elseguest` add branches to the same block.

```html
@auth('staff')
    <p>Staff</p>
@elseguest
    <p>Sign in</p>
@endauth
```

### Authorization Directives {#authorization-directives}

`@can` checks that the current user has every listed ability. `@cannot` selects the branch when that check fails. `@canany` checks whether any listed ability is allowed. Use `@elsecan`, `@elsecannot`, `@elsecanany`, or `@else` for alternate branches. Close the block with the ending directive that matches its opening directive.

The directives use the current HTTP request’s Gate, including guest abilities and policy hooks. Pass a view value or an ordered list as the second argument. Scalars and values inside that list are passed as `ViewValue`. To select a Rust policy, attach the resource with `View::withGateArgument` under a dedicated name. Use `PolicyTarget::of::<T>()` for a policy without a resource instance. Keep route or handler authorization in place for the protected action.

```html
@can('update', $policyArticle)
    <a href="/articles/edit">Edit</a>
@elsecanany(['preview', 'share'], [$policyArticle])
    <a href="/articles/preview">Preview</a>
@endcan

@cannot('delete', $policyArticle)
    <p>Deletion is restricted.</p>
@endcannot
```

```rust
use openrust::view::{View, view};

struct Article {
    title: String,
}

fn article_page(article: Article) -> View {
    view("articles.show")
        .with("title", article.title.clone())
        .withGateArgument("policyArticle", article)
}
```

### Environment, Session, and Context {#environment-session-context}

Use `@production` and `@env` to match the application environment. `@env` accepts a name or list, including wildcard patterns. `@session` and `@context` expose `$value` inside their blocks. Session values support dotted keys, and `@context` reads the public request context.

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

### Including Subviews {#including-subviews}

Pass a map to override inherited data. `@includeFirst` checks candidate values in insertion order, using a list or keyed map written in the template or supplied by the view. It renders the first existing view and reports an error when none exists. `@includeIsolated` receives its explicit values and the view factory’s shared data.

```html
@includeFirst(['custom' => 'custom.notice', 'default' => 'partials.notice'], ['label' => $title])
@includeIsolated('partials.notice', ['label' => 'Opening soon'])
```

Render one partial per entry with `@each`. The partial gets `$key`, the named item, and shared view data. A fourth argument can name an empty-state view. Use the `raw|` prefix for a trusted literal fallback.

```html
@each('partials.job', $jobs, 'job', 'partials.empty')
```

`@each` also accepts collections registered with `withExactSizeIterator` or `withIterator`. An exact-size iterator supplies its count without reading any entries and renders on that iterator. An unknown-length iterator is consumed to count its entries. If non-empty, Blade opens a fresh iterator to render the rows, following Laravel’s `LazyCollection` behavior. Supply a finite iterator factory that can reopen the collection. Empty fallbacks receive shared data only.

### Expressions and Conditional Attributes {#conditional-attributes}

Use parentheses, `!`, `&&`, `||`, `===`, `!==`, and `??` in conditions and echoes. Strict comparison preserves scalar types. PHP object identity and loose coercion have no corresponding operation in the view data model.

Use `@class` and `@style` with a literal or a runtime value such as `$classes` or `$styles`. Lists and numeric keys supply unconditional entries. Other keys supply entries when their values are truthy. Entries keep their input order and each style ends in one semicolon. `@checked`, `@selected`, `@disabled`, `@readonly`, and `@required` emit their HTML attribute when true.

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

### Array Expressions {#array-expressions}

Use `[value, key => value]` or `array(...)` to construct an array. Keys and values can use variables, nested arrays and supported operators. Use `$items[$key]` to read an element. Missing elements fail rendering unless accessed through `??`, `@isset` or `@empty`. Entries keep their insertion order, and a repeated key replaces its value in place. Implicit integer keys follow the largest existing integer key, starting at zero for a new array.

```html
<p>{{ ['colors' => [$primary, 'blue']]['colors'][0] ?? 'blue' }}</p>
```

Canonical integer strings, booleans, null and in-range floating-point keys follow PHP key conversion. Lists with consecutive zero-based indices use native view arrays. Other arrays become ordered view objects with string keys. Unsupported key values and an implicit index overflow fail rendering. Prepare PHP-specific references and overloaded object access in application Rust code.

Use `...$items` inside an array expression to expand a native list or ordered view object. Integer keys are assigned new consecutive indices after the current largest integer key; string keys replace earlier values without changing their position. Expansion copies values and leaves its source unchanged. Scalars fail rendering. Materialize PHP Traversable equivalents as native view arrays or objects in Rust before expanding them.

```html
{{ [...['color' => 'blue'], ...['color' => 'red']]['color'] }}
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

`withIterator` attaches a Rust iterator factory to a variable used by `@foreach` or `@forelse`. The factory returns key-value pairs. For an iterator of unknown length, `$loop->count`, `remaining`, and `last` are null. Use `withExactSizeIterator` when the iterator can report its length without advancing. Every loop starts a fresh iterator, and `@break` stops consumption immediately.

```rust
use openrust::view::{View, view};

fn activity() -> View {
    view("activity").withIterator("items", || {
        ["first", "second"].into_iter().enumerate()
            .map(|(key, value)| (key.into(), value.into()))
    })
}
```

```html
@foreach ($items as $key => $item)
    <p>{{ $key }}: {{ $item }}</p>
    @break
@endforeach
```

### Fonts {#fonts}

Use `@fonts` in the layout head to render preload links and CSS from the version 1 Vite font manifest. The default production location is `public/build/fonts-manifest.json`. When `public/hot` exists, Blade reads `public/fonts-manifest.dev.json`. These paths follow the application’s public directory. A missing manifest emits an empty string. Pass an alias or a list to load only the selected families.

```html
<head>
    @fonts(['sans', 'mono'])
</head>
```

## Components

Store an anonymous component such as `notice` in `resources/views/components/notice.blade.html`. A quoted attribute is a string. Prefix its name with `:` to pass a view expression. Render the default slot with `$slot` and access a named slot by its name. Slot content is already rendered HTML. Data echoed inside the slot is escaped normally.

```html
<aside><strong>{{ $heading }}</strong>{{ $slot }}</aside>
```

Use `:$title` as shorthand for `:title="$title"`. A `::` prefix preserves a client-side binding such as `:class`. Components under a registered view namespace use tags such as `<x-demo::panel>`.

A registered Rust component can override `Component::shouldRender` to hide itself and its slots.

### Generating Components {#generating-components}

The default command creates a Rust component under `app/view/components` and a template under `resources/views/components`. Use `--inline` to store the template in the Rust component or `--view` to generate an anonymous component. For templates, `--path` selects a directory relative to `resources/views`. Add a generated Rust component to your module tree and register it with `Factory::component`.

```shell
./artisan make:component Forms/InputField
./artisan make:component InlineNotice --inline
./artisan make:component EmptyState --view
```

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

Chain attribute bag methods from left to right. `whereStartsWith` and its alias `thatStartWith` keep matching attribute names. `whereDoesntStartWith` excludes them. Use a string or a list of prefixes. The same methods work with slot attribute bags.

```html
<aside {{ $attributes->whereStartsWith(['data-', 'aria-'])->class(['panel']) }}>
    {{ $slot }}
</aside>
```

Component and slot tags also accept `@class($classes)` and `@style($styles)`. To filter, merge or forward a bag, call `class($classes)` and `style($styles)` on it. These methods prepend the generated defaults to the caller’s class and style. HTML attribute values are escaped when rendered.

```html
<div @class($classes) @style($styles)>{{ $label }}</div>
<x-notice {{ $attributes->class($classes)->style($styles) }} />
```

#### Reading Attributes and Slots {#filtering-attributes}

Use `first` or `get` to read a value, and `has` or `hasAny` to check keys. Use `prepends` inside `merge` to prepend a default to an attribute. Register application-specific predicates with `BladeCompiler::attributeFilter` and pass the registered name to `filter`. A slot supports `isEmpty()`, `isNotEmpty()`, and `hasActualContent()`. The last method ignores HTML comments. Rust components expose properties through `Component::data`. The template reserves `$attributes` and `$slot` for the attribute bag and default slot.

```html
<div {{ $attributes->merge(['data-controller' => $attributes->prepends('profile')]) }}>
    @if ($attributes->has('aria-label'))
        {{ $attributes->get('aria-label') }}
    @endif
    @if ($slot->hasActualContent())
        {{ $slot }}
    @endif
</div>
```

#### Scoped Slots {#scoped-slots}

A registered Rust component can expose view values through `Component::data`. A slot reads those values through `$component`, and the component template reads the same values as ordinary variables. Compute values from component methods in Rust before returning them.

```html
<x-scoped-panel>
    <x-slot:heading>{{ $component->caption }}</x-slot:heading>
</x-scoped-panel>
```

### Accessing Parent Component Data {#component-parent-data}

Declare `@aware` at the start of the child template, alongside `@props`. The nearest component's explicitly passed value wins. A default defined only by the parent's `@props` stays local to that parent.

```html
@aware(['color' => 'neutral'])
<span>{{ $color }}</span>
```

### Anonymous Component Locations {#anonymous-component-paths}

Register additional anonymous component roots during application setup. A view namespace points to a directory under the view roots. A component path points to a filesystem directory. `view:cache` also compiles registered paths.

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

Use `@hasSection` or `@sectionMissing`, closed with `@endif`, to test section content. An optional second argument supplies the same default as `@yield`. A defined section takes precedence, even when empty. After trimming PHP’s standard whitespace, empty output and the string `0` count as missing.

```html
@hasSection('content', $fallback)
    @yield('content', $fallback)
@endif
```

Use `@extendsFirst` to select the first available layout. Both inheritance directives accept a map of explicit view data. Use `@append` or `@overwrite` to change how the current section is stored. `@stop` closes a section like `@endsection`.

```html
@extendsFirst(['themes.card', 'layouts.app'], ['site' => 'OpenRust'])
```

## Forms {#forms}

In a session-enabled web route, `@csrf` emits the current session token as a hidden `_token` field. `@error` renders the first message for the field and makes it available as `$message` inside the block. Pass a second argument to read a named error bag.

```html
<form method="post">
    @csrf
    @method('PUT')
    @error('name')
        <span>{{ $message }}</span>
    @enderror
</form>
```

A browser POST form can target a PUT, PATCH, or DELETE route. Put `@method('PUT')` after `@csrf` to emit the hidden `_method` field. The router uses the effective method and the web middleware still checks CSRF. In a handler, `request.getRealMethod()` returns the original transport method.

For POST requests, the `X-HTTP-Method-Override` header takes precedence over the form field, with the query parameter used as a fallback.

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

Use `@pushIf` with a condition and stack name. `@elsePushIf` can select another conditional capture.

```html
@pushIf($showAssets, 'scripts')
    <script src="/assets/extra.js" defer></script>
@endPushIf
```

## Rendering Blade Fragments {#rendering-blade-fragments}

Mark part of a template with `@fragment` and `@endfragment`. Return `view("dashboard").fragment("summary")` from the route to send that fragment.

```html
@fragment('summary')
<h2>{{ $title }}</h2>
@endfragment
```

`fragments(["summary", "details"])` returns the named fragments in the given order. `allFragments()` returns every captured fragment. Use `fragmentIf(condition, "summary")` or `fragmentsIf(condition, names)` to keep the full template when the condition is false. Each name triggers a render of the same view in order, including duplicate names. The composer runs each time. A missing name returns the full output of that render. An empty name list performs no renders. `allFragments()` collects fragments from one render.

Named fragment buffers retain their own leading whitespace. A missing name falls back to the full view after PHP leading-whitespace removal. `allFragments()` corresponds to Laravel's public `fragments()` call without a names argument; PHP's internal `allFragments` method is protected.

## Service Injection {#service-injection}

Register a typed service with `Factory::registerViewService` and return the data the template needs. Put `@inject` at the start of the template to bind a registered service alias to a variable.

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

### Custom Echo Handlers {#custom-echo-handlers}

Register a formatter for a Rust type with `stringable` on the application’s Blade compiler. Pass a typed value with `View::withStringable`. The formatter produces text, so regular Blade echo escaping still applies. Use `stringableIterable` to register a formatter for arrays and maps echoed in templates.

```rust
use openrust::view::{View, ViewResult, compilers::BladeCompiler, view};

struct Price(u16);

fn configure(compiler: &BladeCompiler) {
    compiler.stringable(|price: &Price| Ok(format!("USD {}", price.0)));
}

fn checkout() -> ViewResult<View> {
    view("checkout").withStringable("price", &Price(42))
}
```

```html
<strong>{{ $price }}</strong>
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

### Expression Runtime and Compilation {#expression-runtime}

PHP templates can call arbitrary PHP functions and objects. OpenRust compiles a Rust instruction tree with the following explicit mappings.

| PHP form | Rust equivalent |
| --- | --- |
| Function, static method or callable expression | Compute its value in a handler, `Factory::creator` or `Factory::composer`. For output at rendering time, register `BladeCompiler::directive`; for a predicate use `r#if` and `check`. Calls such as `strtoupper($name)` are rejected inside expressions. |
| References and by-reference iteration | Perform mutation and alias-sensitive work in Rust, then supply `ViewData`. ViewValue arrays contain owned values and do not create PHP aliases. |
| Array unpacking and Traversable | Use `...` with native arrays or ordered objects. Materialize an iterator in Rust; `withIterator` supplies foreach iteration and is not an array-unpacking source. |
| ArrayAccess, magic properties and arbitrary object methods | Project fields to ViewValue arrays or objects, use `Factory::registerViewService` with `@inject`, or expose computed component values through `Component::data`. Typed text output uses `stringable` and `View::withStringable`. |
| PHP statements and imports | Use Rust code and module imports. Unregistered `@php`, `@endphp` and `@use` fail compilation. Escape them with `@@` or place them in `@verbatim` when literal output is intended. |

`view:cache` parses every branch and publishes the instruction tree without running view creators, composers, directive callbacks or component factories. Unsupported syntax therefore fails compilation even in an unselected branch. Valid array expressions evaluate their inputs when rendered: a scalar unpacking source fails only if that branch runs. A failed render does not change the cached tree or subsequent data. Registrations belong to the application compiler; replacing a callback keeps cached templates using the current callback. Register directive and condition names before precompilation.

Prepare values that require PHP function calls or `@php` blocks in a Rust handler, view creator, or composer, then pass them to the view. For PHP class components and dynamic class discovery, register the corresponding Rust `Component` factory with `Factory::component`. Anonymous components resolve through the view finder.

Use `@once` for content that should render only once per view tree. `@pushOnce` and `@prependOnce` also accept an optional ID shared across templates.
