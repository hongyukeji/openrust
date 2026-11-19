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

### 算术表达式 {#arithmetic-expressions}

输出、条件、数组元素、包含视图和组件属性可使用 `+`、`-`、`*`、`/`、`%` 和 `**`。乘法优先于加法；幂运算从右向左结合，优先于一元负号。

```html
{{ 1 + 2 * 3 }}
{{ (1 + 2) * 3 }}
{{ 2 ** 3 ** 2 }}
@if ($price * $quantity >= 100)Bulk order @endif
```

前三行分别输出 `7`、`9` 和 `512`。有符号 64 位整数溢出后转为双精度浮点数。数值字符串、布尔值和 null 按 PHP 算术规则转换；非数值操作数、除零和未定义变量使渲染失败。`??`、`@isset` 和 `@empty` 允许值缺失；`&&`、`||` 和 `??` 保持短路。失败响应由应用异常策略处理。

浮点取模会将操作数截断为整数；零底数的负幂产生无穷大。这两类运算按 Laravel 异常引导机制发出弃用提示，继续渲染。通过 `logging.deprecations.channel` 保留提示；日志失败不会中断渲染。testing 环境默认跳过，`LOG_DEPRECATIONS_WHILE_TESTING` 为真时启用。可选 trace 记录原生 Rust 调用栈和异常快照；来源位置指向原生求值器，不冒充 PHP 编译模板。

直接标量输出、算术、真假判断和比较保留 `INF`、`NAN` 及有符号零。视图数据仍使用 JSON，非有限数不能传入数组、包含视图、组件数据和自定义 JSON 回调；这类数据需要时在应用 Rust 代码中准备。模板不执行任意 PHP 或调用未注册函数。编译器会自动使旧表达式缓存失效。

### HTML 实体编码 {#html-entity-encoding}

Blade 默认会对转义输出中的已有 HTML 实体再次编码。要保留有效的 HTML 4.01 实体，可在应用服务提供者启动时配置 Blade 编译器。调用 `withDoubleEncoding()` 可恢复默认行为。

```rust
use openrust::view::compilers::BladeCompiler;

fn configure_encoding(compiler: &BladeCompiler) {
    compiler.withoutDoubleEncoding();
}
```

### 输出 JavaScript 数据 {#rendering-json}

在脚本中用 `Js::from` 或 `@js` 将视图值序列化为 JavaScript 表达式。渲染器转义 HTML 敏感字符、引号和行分隔符。

```html
<script>
    const settings = {{ Js::from($settings) }};
</script>
```

## Blade 指令

用 `@if`、`@elseif`、`@else`、`@endif`、`@unless` 和 `@endunless` 判断视图数据。用 `@foreach ($items as $item)` 和 `@endforeach` 遍历。表达式支持标量字面量、变量及 `$user->name` 等对象属性。OpenRust 使用自己的模板编译器与 Rust 编译产物格式。 可用 `@isset` 和 `@empty` 检查数据，`@forelse` 可以通过 `@empty` 处理空集合。

用 `@includeIf('partials.notice')` 跳过缺失的视图。用 `@includeWhen($showNotice, 'partials.notice')` 或 `@includeUnless($hideNotice, 'partials.notice')` 按条件渲染。子视图继承当前视图数据，包括循环变量。条件成立并选中视图时，`@includeWhen` 和 `@includeUnless` 要求该视图存在。

### 指令边界与空白 {#directive-whitespace}

内置指令名按 Laravel 编译方法忽略大小写，注册的自定义名称仍区分大小写。指令前应与 ASCII 字母、数字或下划线隔开。例如 `word@if(...)` 是文本，`<b>word</b>@if(...)` 才开始条件。HTML 注释和标点可以提供边界；Blade 注释不能隔开前面的 ASCII 单词与指令。非 ASCII 文本不属于 ASCII 单词字节。

```html
@if (true)
    <b>Visible</b>
@endif
```

指令的左括号应与名称同行，中间允许空格或制表符。未知指令及未知的 `name::member` 名称原样保留。`@@if` 输出 `@if`，单独的 `@@` 保持不变。大段原样内容使用 `@verbatim`。

Blade 复现 PHP 关闭标签的行为：生成的操作会吃掉紧随其后的一个 LF、CRLF 或 CR；换行前有空格或制表符时则保留。转义及原样输出表达式会保留其后的换行。每个编译视图移除开头的 PHP 空白字节（空格、制表符、LF、CR、垂直制表符和 NUL），换页符和不换行空格保留。普通 `.html` 视图保留原始内容。旧编译产物会自动失效。

### 注释与原样块编译 {#source-preparation}

Blade 先保护完整的 `@verbatim` 块，再从整个源码移除 <code v-pre>{{-- ... --}}</code> 注释。字符串表达式、指令名称和参数、组件属性中的注释同样会被删除。注释会连接两侧的标记，不充当空白。

```html
@i{{-- note --}}f (true)
    {{ 'a{{-- note --}}b' }}
    {{ 1 {{-- note --}} + 2 }}
@endif
```

两个回显分别输出 `ab` 和 `3`。原有原样块中的注释、回显和组件标签保持字面内容。删除注释后拼成的原样块标记不会被重新保护。`@verbatim` 后的开头空白仍遵循前述关闭标签换行规则。没有完整分隔符的未闭合注释、回显和原样块标记按文本保留。字符串表达式中的原样内容会在表达式求值及转义前恢复。旧编译产物自动重建，无需 PHP 运行时。

### 认证指令 {#authentication-directives}

在 HTTP 视图中用 `@auth` 和 `@guest` 检查配置的守卫。传入名称可以指定守卫。`@elseauth` 和 `@elseguest` 在同一区块中增加分支。

```html
@auth('staff')
    <p>Staff</p>
@elseguest
    <p>Sign in</p>
@endauth
```

### 授权指令 {#authorization-directives}

`@can` 检查当前用户是否拥有列出的全部能力。`@cannot` 在该检查失败时选中分支。`@canany` 检查是否拥有任一能力。可使用 `@elsecan`、`@elsecannot`、`@elsecanany` 或 `@else` 选择后续分支。使用与起始指令配对的结束指令闭合区块。

指令使用当前 HTTP 请求的 Gate，可以求值访客能力并执行策略回调。第二个参数可传入视图值或有序列表，普通标量和列表元素均以 `ViewValue` 传给 Gate。要选择 Rust 策略，用 `View::withGateArgument` 以专用名称附加资源。无实例的策略可传入 `PolicyTarget::of::<T>()`。受保护操作仍应在路由或处理器中校验授权。

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

### 环境、会话与上下文 {#environment-session-context}

用 `@production` 和 `@env` 匹配应用环境。`@env` 接受单个名称或列表，支持通配符。`@session` 和 `@context` 在各自区块内提供 `$value`。会话支持点分键，`@context` 读取公开的请求上下文。

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

### 包含子视图 {#including-subviews}

传入映射可覆盖继承的视图数据。`@includeFirst` 按插入顺序检查候选值，可接收模板中的列表或带键映射，也可接收视图变量。它渲染首个存在的视图，全部缺失时报告错误。`@includeIsolated` 只接收显式数据和视图工厂的共享数据。

```html
@includeFirst(['custom' => 'custom.notice', 'default' => 'partials.notice'], ['label' => $title])
@includeIsolated('partials.notice', ['label' => 'Opening soon'])
```

`@each` 为每个条目渲染一个子视图。子视图可读取 `$key`、指定名称的条目和视图共享数据。第四个参数可指定空集合视图，也可用 `raw|` 前缀传入可信的字面量。

```html
@each('partials.job', $jobs, 'job', 'partials.empty')
```

`@each` 也接受通过 `withExactSizeIterator` 或 `withIterator` 注册的集合。已知长度的迭代器直接提供条目数，并使用同一个迭代器渲染。未知长度的迭代器先完整读取以计数，非空时重新打开迭代器渲染，沿用 Laravel `LazyCollection` 的行为。请提供有限且可重新打开的迭代器工厂。空集合回退视图只接收共享数据。

### 表达式与条件属性 {#conditional-attributes}

条件和输出表达式可使用括号、`!`、`&&`、`||`、`===`、`!==` 和 `??`。严格比较保留标量类型。PHP 的对象身份和宽松类型转换超出视图数据模型的范围。

`@class` 和 `@style` 接受字面量或 `$classes`、`$styles` 一类运行时值。列表项和数字键的值直接输出，其他键在对应值为真时输出。条目保持输入顺序，每条样式以一个分号结尾。`@checked`、`@selected`、`@disabled`、`@readonly` 和 `@required` 在条件为真时输出对应的 HTML 属性。

```html
<input @checked($ready && $count === 2) @class(['field', 'active' => $ready])>
{{ $label ?? 'Pending' }}
```

`@for` 使用有符号整数计数器，支持 `++`、`--`、`+=` 和 `-=`。`@while` 在条件为真时重复。`@break` 和 `@continue` 可接收条件或字面量嵌套层数。每次渲染中，计数循环与 `@while` 共用 100,000 次迭代上限。计数器的作用域为当前循环。

```html
@for ($index = 0; $index < 3; $index++)
    @continue($index === 1)
    {{ $index }}
@endfor
```

使用 `@switch`、`@case`、`@default` 与 `@endswitch` 按视图值分支。选中的分支会依次执行后续分支，直到 `@break` 或整个区块结束。数字字符串可匹配数字分支。`@bool` 输出 `true` 或 `false` 文本。

```html
@switch($state)
    @case('ready')
        Ready
        @break
    @default
        Pending
@endswitch
```

### 数组表达式 {#array-expressions}

用 `[value, key => value]` 或 `array(...)` 构造数组。键和值可以使用变量、嵌套数组及已支持的运算符。用 `$items[$key]` 读取元素，缺失时使渲染失败，`??`、`@isset` 和 `@empty` 访问除外。条目按插入顺序保留，重复键就地替换值。隐式整数键从零开始，之后接在当前最大整数键后。

```html
<p>{{ ['colors' => [$primary, 'blue']]['colors'][0] ?? 'blue' }}</p>
```

规范整数文本、布尔值、null 和浮点键按 PHP 规则转换。连续且从零开始的列表保留为 JSON 数组，其他数组表示为带字符串键的有序视图对象。无效键和溢出的隐式索引使渲染失败。引用、展开和对象重载访问由应用 Rust 代码准备。

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

`withIterator` 为 `@foreach` 或 `@forelse` 的变量注册 Rust 迭代器工厂，工厂返回键值对。长度未知时，`$loop->count`、`remaining` 和 `last` 为 null。可在不推进迭代的情况下获取长度时，用 `withExactSizeIterator`。每次执行循环都会创建新迭代器，`@break` 立即停止读取。

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

### 字体 {#fonts}

在布局的 head 中使用 `@fonts`，从版本 1 的 Vite 字体清单输出预加载链接和 CSS。生产清单默认为 `public/build/fonts-manifest.json`。存在 `public/hot` 时，Blade 读取 `public/fonts-manifest.dev.json`。这些路径随应用的公开目录配置。清单缺失时输出空字符串。可以传入单个别名或别名列表来选择字体。

```html
<head>
    @fonts(['sans', 'mono'])
</head>
```

## 组件

将匿名组件 `notice` 放在 `resources/views/components/notice.blade.html`。带引号的属性是字符串，属性名前加 `:` 可以传入视图表达式。默认插槽用 `$slot` 读取，命名插槽用对应名称读取。插槽内容是已渲染的 HTML，插槽内输出的数据仍按普通规则转义。

```html
<aside><strong>{{ $heading }}</strong>{{ $slot }}</aside>
```

`:$title` 是 `:title="$title"` 的简写。`::` 前缀保留 `:class` 等客户端绑定。已注册视图命名空间的组件可用 `<x-demo::panel>` 访问。

已注册的 Rust 组件可通过 `Component::shouldRender` 控制组件和插槽是否渲染。

### 生成组件 {#generating-components}

默认命令在 `app/view/components` 生成 Rust 组件，在 `resources/views/components` 生成模板。`--inline` 将模板写入 Rust 组件，`--view` 生成匿名组件。`--path` 可指定相对于 `resources/views` 的模板目录。将生成的 Rust 组件加入应用的模块树，并通过 `Factory::component` 注册。

```shell
./artisan make:component Forms/InputField
./artisan make:component InlineNotice --inline
./artisan make:component EmptyState --view
```

### 插槽属性 {#slot-attributes}

可以用 `<x-slot:footer-actions>` 或 `<x-slot name="footerActions">` 声明带属性的插槽。通过 `$footerActions->attributes` 读取属性包，可调用 `merge`、`class`、`style`、`only` 和 `except`。

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

### 组件属性与属性包 {#component-attributes}

在组件模板开头用 `@props` 声明属性和默认值。调用方可以用 `display-name` 向 `displayName` 传值。声明之外的 HTML 属性可以通过 `$attributes` 读取。

```html
@props(['displayName' => 'Notice', 'variant',])
<aside {{ $attributes->merge(['class' => 'panel', 'role' => 'status']) }}>
    {{ $displayName }}{{ $slot }}
</aside>
```

使用 `{{ $attributes }}` 输出属性包。`merge` 添加默认值，普通属性以调用方的值为准，`class` 和 `style` 合并默认值与调用方的文本。HTML 属性值会转义，null 和 false 会省略。

属性包方法从左到右执行。`whereStartsWith` 和别名 `thatStartWith` 保留名称匹配的属性，`whereDoesntStartWith` 排除匹配项。可以传入单个前缀或前缀列表。插槽属性包也支持这些方法。

```html
<aside {{ $attributes->whereStartsWith(['data-', 'aria-'])->class(['panel']) }}>
    {{ $slot }}
</aside>
```

组件和插槽标签同样接受 `@class($classes)` 和 `@style($styles)`。可在属性包上调用 `class($classes)`、`style($styles)` 再筛选、合并或向子组件传递。生成的默认值放在调用方的 class 和 style 之前。渲染时会转义 HTML 属性值。

```html
<div @class($classes) @style($styles)>{{ $label }}</div>
<x-notice {{ $attributes->class($classes)->style($styles) }} />
```

#### 读取属性与插槽 {#filtering-attributes}

使用 `first` 或 `get` 读取属性值，使用 `has` 或 `hasAny` 检查属性键。在 `merge` 中使用 `prepends` 可将默认值加在属性值开头。应用可通过 `BladeCompiler::attributeFilter` 注册 Rust 谓词，并将注册名称传给 `filter`。插槽支持 `isEmpty()`、`isNotEmpty()` 和 `hasActualContent()`。最后一种判断会忽略 HTML 注释。Rust 组件通过 `Component::data` 显式提供属性。`$attributes` 和 `$slot` 由模板保留给属性包和默认插槽。

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

#### 作用域插槽 {#scoped-slots}

已注册的 Rust 组件可通过 `Component::data` 提供视图值。插槽通过 `$component` 读取这些值，组件模板通过普通变量读取同一份数据。组件方法的结果在 Rust 中计算后传入。

```html
<x-scoped-panel>
    <x-slot:heading>{{ $component->caption }}</x-slot:heading>
</x-scoped-panel>
```

### 读取父组件数据 {#component-parent-data}

在子组件模板开头使用 `@aware`，可与 `@props` 一起声明。最近一层组件显式传入的值优先。父组件通过 `@props` 声明的默认值只用于该父组件。

```html
@aware(['color' => 'neutral'])
<span>{{ $color }}</span>
```

### 匿名组件目录 {#anonymous-component-paths}

在应用装配时注册额外的匿名组件目录。通过命名空间指向视图根下的目录，或通过路径指定文件系统目录。`view:cache` 也会编译已注册路径中的模板。

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

用 `@hasSection` 或 `@sectionMissing` 判断区段内容，并用 `@endif` 结束。可选的第二个参数与 `@yield` 的默认值相同。已定义的区段优先，即使其内容为空。按 PHP 的规则裁剪空白后，空输出和字符串 `0` 都视为缺失。

```html
@hasSection('content', $fallback)
    @yield('content', $fallback)
@endif
```

用 `@extendsFirst` 选择第一个可用布局。两个继承指令都可显式传入视图数据。用 `@append` 追加区段或用 `@overwrite` 覆盖区段。`@stop` 与 `@endsection` 都可结束区段。

```html
@extendsFirst(['themes.card', 'layouts.app'], ['site' => 'OpenRust'])
```

## 表单 {#forms}

在启用 Session 的 Web 路由中，`@csrf` 将当前会话令牌输出为隐藏的 `_token` 字段。`@error` 在有字段错误时渲染区块，并在块内用 `$message` 提供首条消息。第二个参数可指定命名错误包。

```html
<form method="post">
    @csrf
    @method('PUT')
    @error('name')
        <span>{{ $message }}</span>
    @enderror
</form>
```

浏览器 POST 表单可以提交到 PUT、PATCH 或 DELETE 路由。在 `@csrf` 后写入 `@method('PUT')`，即可输出隐藏的 `_method` 字段。路由按覆盖后的方法匹配，Web 中间件仍校验 CSRF。处理器可通过 `request.getRealMethod()` 读取原始请求方法。

POST 请求优先使用 `X-HTTP-Method-Override` 请求头，其次使用表单字段，最后读取查询参数。

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

`@pushIf` 接收条件和栈名。可用 `@elsePushIf` 添加另一个条件分支。

```html
@pushIf($showAssets, 'scripts')
    <script src="/assets/extra.js" defer></script>
@endPushIf
```

## 渲染 Blade 片段 {#rendering-blade-fragments}

用 `@fragment` 和 `@endfragment` 标记模板片段。从路由返回 `view("dashboard").fragment("summary")` 即可发送该片段。

```html
@fragment('summary')
<h2>{{ $title }}</h2>
@endfragment
```

`fragments(["summary", "details"])` 按参数顺序返回指定片段。`allFragments()` 返回全部已捕获片段。可以用 `fragmentIf(condition, "summary")` 或 `fragmentsIf(condition, names)` 在条件为假时发送完整模板。每个名称都会按顺序重新渲染同一个视图，重复名称也会触发渲染。组合器在每次渲染时运行。名称缺失时返回该次渲染的完整输出。空名称列表不触发渲染。`allFragments()` 从一次渲染中收集所有片段。

命名片段缓冲区保留自身的开头空白；名称缺失时回退到经过 PHP 开头空白清理的完整视图。`allFragments()` 对应 Laravel 不传名称参数的公开 `fragments()` 调用，PHP 内部的 `allFragments` 方法受保护。

## 服务注入 {#service-injection}

通过 `Factory::registerViewService` 注册类型化服务，并返回模板所需的数据。在模板开头用 `@inject` 将已注册的服务别名绑定到变量。

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

## 渲染内联 Blade 模板 {#rendering-inline-blade-templates}

`Factory::renderInline(source, &data)` 在内存中渲染受信任的模板文本。子视图由同一个工厂解析。

## 扩展 Blade {#extending-blade}

在应用装配时调用 `BladeCompiler::directive` 注册自定义指令。回调接收求值后的视图数据。`DirectiveOutput::Text` 会转义，`DirectiveOutput::Html` 作为可信 HTML 输出。

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

### 自定义输出处理器 {#custom-echo-handlers}

在应用的 Blade 编译器上通过 `stringable` 为 Rust 类型注册格式化函数。通过 `View::withStringable` 传入类型化数据。格式化结果是文本，继续遵循 Blade 的输出转义规则。使用 `stringableIterable` 可为模板中直接输出的数组和映射注册格式化函数。

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

### 自定义条件 {#custom-conditions}

使用 `BladeCompiler::r#if` 注册谓词。谓词接收求值后的视图数据，也可以在 Rust 中用 `check` 调用同一谓词。

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

## PHP 与 Rust 模板 {#php-and-rust-templates}

PHP 函数调用或 `@php` 代码块需要的值，可以在 Rust 处理器、视图创建器或组合器中准备，再传给视图。PHP 类组件及其动态类发现对应 `Factory::component` 显式注册的 Rust `Component` 工厂。匿名组件从视图查找器解析。

用 `@once` 在同一渲染树中只输出一次内容。`@pushOnce` 和 `@prependOnce` 也接受可跨模板共用的可选标识。
