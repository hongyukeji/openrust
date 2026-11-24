# Validation

## Manually Creating Validators

Create a validator with `Validator::make` and call `validate().await`. The returned input contains the validated fields. Use dot paths and `*` to select nested elements. Use `nullable` to allow an explicit null and `sometimes` to apply rules only when a key is present.

```rust
use openrust::{
    contracts::validation::Validator as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Validator,
    validation::{Data, RuleSet},
};

fn store(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Data::new(request.input(None).await?.unwrap_or_default())?;
        let rules = RuleSet::parse([
            ("name", "required|string|min:2"),
            ("users.*.email", "required|string"),
        ])?;
        let input = Validator::make(data, rules)?.validate().await?;
        Response::json(StatusCode::OK, input.into_value())
    })
}
```

`email` and `email:rfc` accept warning-only RFC addresses. `strict` rejects RFC parser warnings; `filter` and `filter_unicode` use native-style filtering, with the latter allowing letters and numbers from Unicode in the local part. `spoof` checks the highly restrictive Unicode script profile. `dns` checks mail-domain DNS, including MX rejection records and address-record fallback; it does not verify mailbox existence. Combine modes with commas to require all of them. DNS or spoof alone does not add RFC syntax validation. Unrecognized mode strings fall back to RFC validation.

## Validating incoming requests {#validating-incoming-requests}

Call `request.validate(rules).await?` directly in a handler. It uses the application validator and the current request, returning only validated input. Validation failures use the normal JSON 422 or Web redirect/session error pipeline.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::RuleSet,
};

fn store(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::parse([("title", "required|string|max:255")])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

Use `request.validate_with_bag("post", rules).await?` for a named error bag. The Request facade delegates to the same instance methods. In validation middleware, Precognition applies `Precognition-Validate-Only` and returns a successful preflight before subsequent business logic. Keep ordinary route actions out of preflight execution; use a Form Request or validation middleware for precognitive validation.

## Database Validation Rules {#database-validation-rules}

Use `exists:table,column` to require a matching row and `unique:table,column` to reject an existing value. Omit the column to use the field name and prefix the table with `connection.` to select a named database connection. Both rules use the write connection. For an array, `exists` uses one query to check that all distinct values exist.

In string rules, quote parameter values that contain commas, for example `exists:directory.references,reference,namespace,"team,west"`. A text exclusion ID such as `001` keeps its leading zeros. Array existence checks use scalar string representations to count distinct inputs.

```rust
use openrust::validation::{Rule, RuleSet, ValidationResult};

fn account_rules() -> ValidationResult<RuleSet> {
    RuleSet::parse([
        ("account_id", "required|integer|exists:directory.accounts"),
        ("ref", r#"exists:directory.references,reference,namespace,"team,west""#),
    ])?
        .field("email", [
            Rule::unique("directory.accounts", Some("email"))
                .without_trashed(None)
                .into(),
        ])
}
```

Fluent rules support query constraints and soft-deleted rows. When updating a row, use `ignore` with a trusted identifier or `ignore_model` with a model already resolved and authorized by the application. Use a database unique constraint to enforce uniqueness across concurrent writes.

`r#where` accepts a column or `DatabaseWhere::callback`, and a scalar or an array of typed bindings. Arrays become `where_in`; `where_not` with an array becomes `where_not_in`, including empty arrays. Pass `Value::Null` for the optional value when supplying a callback or a null constraint: Rust cannot overload method arity or omit ordinary arguments. Callbacks reuse the existing nested query conditions; the explicit null placeholder adds no SQL condition. The named `directory` database connection must be configured before using this example.

```rust
use openrust::{database::Value, validation::{Rule, RuleSet, ValidationResult, rules::DatabaseWhere}};
fn directory_rules() -> ValidationResult<RuleSet> {
    RuleSet::new()
        .field("id", [Rule::exists("directory.accounts", Some("account_id"))
            .r#where(DatabaseWhere::callback(|query| query.r#where("tenant", "=", 7).or_where("tenant", "=", 8)), Value::Null)
            .where_null("deleted_at").into()])?
        .field("email", [Rule::unique("directory.accounts", Some("email"))
            .r#where("tenant", [7, 8]).where_null("deleted_at").into()])
}
```

## Form Request Validation {#form-request-validation}

### Creating a form request {#creating-form-requests}

```sh
./artisan make:request StoreArticleRequest
./artisan make:request Admin/StoreArticleRequest
```

The command creates `app/http/requests/store_article_request.rs` (or the nested `admin/` module file). The generated type implements `FormRequest`, derives `Default`, and defines `authorize` and `rules`. Authorization denies by default: explicitly implement your authorization before using the form. Rules start empty, ready for your application’s fields.

Declare `pub(crate) mod requests;` in `app/http/mod.rs`, then declare the generated file module in `app/http/requests/mod.rs` (for example, `pub(crate) mod store_article_request;`). Rust requires compiled module declarations; the generator preserves existing module files. Use `ValidatedFormRequest<StoreArticleRequest>` as the typed action argument.

`-f` / `--force` replaces an existing file. Without it, duplicate generation reports the conflict, preserves the file and exits 0, matching the general generator contract; invalid names and write errors exit 1. `./artisan stub:publish` publishes `stubs/request.rs.stub`; edit it to customize future requests. Nested names accept `/` or `\`; `App/Http/Requests/StoreArticleRequest` is also supported.

Use `ValidatedFormRequest<F>` as a typed action parameter. The router creates the form request, prepares the input, authorizes, validates, and calls `passed_validation` before dispatching the action. An authorization denial returns HTTP 403.

```rust
use openrust::{
    foundation::http::{FormRequest, ValidatedFormRequest},
    http::{HttpError, Response, StatusCode},
    routing::RoutingError,
    support::facades::Route,
    validation::{RuleSet, ValidationResult},
};

#[derive(Default)]
struct StoreUser;

impl FormRequest for StoreUser {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("name", "required|string")])
    }
}

async fn store(form: ValidatedFormRequest<StoreUser>) -> Result<Response, HttpError> {
    Response::json(StatusCode::OK, form.validated().all())
}

fn register() -> Result<(), RoutingError> {
    Route::post("/users", store)
}
```

A form request implements `Default` for construction. A transient container binding for the form type can supply constructor dependencies. Use `ControllerDispatcher::with_form_request` for a controller method or `ResourceController::action_with_form_request` for a resource action. A controller using either adapter also receives the original request and its route parameters. Call `validate_resolved` for explicit validation.

## Validating Files {#validating-files}

Use `Data::from_request` for manual multipart validation. Form requests include uploads automatically. Access validated uploads by dotted path with `validated.file`, including indexed paths such as `attachments.0`. The JSON view represents file leaves as null.

`mimes` and `mimetypes` infer the type from file content. `extensions` checks the client's filename extension. Use both when the content type and filename extension matter. Use `image:allow_svg` or `File::image(true)` to accept SVG. The `size`, `min`, `max` and `between` rules measure files in kilobytes of 1,024 bytes.

Content checks recognize JSON objects and arrays as `application/json`, CSV content as plain text, and empty uploads as `application/x-empty`. Use `extensions:csv` when the client's CSV filename extension matters.

Use `dimensions` to constrain width, height, or aspect ratio. Ratios can be decimals or fractions such as `16/9`. SVG accepted by the image rule bypasses raster dimension checks. Use `encoding:UTF-8` or `File::default().encoding("UTF-8")` to check a file's bytes. Use supported encoding names such as UTF-8, ASCII, UTF-16, UTF-32, SJIS, CP932, or EUC-CN. Choose CP932 or SJIS-win for Windows Shift-JIS extensions.

For single-byte uploads, use `encoding:CP850` or `File::default().encoding("ArmSCII-8")`. CP850 accepts every byte. ArmSCII-8 and the Windows or ISO encodings with undefined positions reject those bytes according to PHP mbstring. Encoding validation preserves the uploaded bytes.

For East Asian double-byte uploads, choose the encoding used by the file. `encoding:BIG-5` and `encoding:EUC-KR` check the standard byte ranges. Use `File::default().encoding("CP950")` or `File::default().encoding("UHC")` for the corresponding Windows extensions. Each encoding rejects incomplete pairs and unassigned byte positions according to PHP mbstring.

Use `encoding:EUC-CN` for its assigned two-byte positions and `encoding:CP936` for the Windows byte repertoire. `encoding:GB18030` and `File::default().encoding("GB18030-2022")` follow PHP mbstring’s byte validation. Both accept the same two-byte and four-byte sequences.

Use `encoding:EUC-JP` or `encoding:eucJP-win` for their respective Japanese byte tables. The fluent file rule also accepts `File::default().encoding("CP51932")` and `File::default().encoding("EUC-JP-2004")`. PHP mbstring defines the assigned two-byte and three-byte positions for each variant.

Shift-JIS file validation also supports `SJIS-mac`, `SJIS-2004`, and the `SJIS-Mobile#DOCOMO`, `SJIS-Mobile#KDDI`, and `SJIS-Mobile#SOFTBANK` variants. Choose the file’s encoding, for example `encoding:SJIS-mac` or `File::default().encoding("SJIS-Mobile#SOFTBANK")`. The SoftBank variant also validates its legacy emoji escape sequences.

Use `encoding:ISO-2022-JP` or `encoding:JIS` to check Japanese escape sequences and the return to ASCII before the end of input. The file rule also accepts `ISO-2022-JP-MS`, `ISO-2022-JP-2004`, `ISO-2022-JP-MOBILE#KDDI`, and `CP50220` through `CP50222`, each with PHP mbstring’s rules for character sets and end state.

For HZ, EUC-TW, and ISO-2022-KR uploads, use `encoding:HZ`, `File::default().encoding("EUC-TW")`, or `encoding:ISO-2022-KR`. PHP mbstring validates the escape and shift sequences of HZ and ISO-2022-KR. Its EUC-TW validation covers the assigned positions in CNS 11643 planes 1, 2, and 14.

The four carrier-specific UTF-8 encodings use standard UTF-8 byte validation. For example, use `encoding:UTF-8-Mobile#DOCOMO` or `File::default().encoding("UTF-8-Mobile#SOFTBANK")`. The other two names are `UTF-8-Mobile#KDDI-A` and `UTF-8-Mobile#KDDI-B`. Validation preserves private-use characters in the original bytes.

For TIFF and ICO uploads, combine `file` with a size rule such as `dimensions:width=100,height=100`. TIFF dimensions come from the first image directory. ICO dimensions come from the last entry with the highest bit depth. These checks read metadata without decoding pixels. The `image` rule excludes TIFF and ICO.

PSD and JPEG 2000 uploads can use the same rules. For a JP2 file, dimensions come from its first root-level codestream. For a raw JPEG 2000 codestream, dimensions come from its SIZ marker. The `image` rule excludes both formats.

The same metadata check covers IFF bitmaps, WBMP, XBM, and SWF with an uncompressed or zlib-compressed body. SWF dimensions come from its stage rectangle.

```rust
use openrust::{
    contracts::validation::Validator as _,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Validator,
    validation::{Data, Rule, RuleSet, rules::File},
};

fn upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Data::from_request(request).await?;
        let rules = RuleSet::new().field("image", [
            Rule::required(),
            File::image(false)
                .dimensions(Rule::dimensions().min_width(256).ratio("1/1"))
                .max("2mb")
                .into(),
        ])?;
        let validated = Validator::make(data, rules)?.validate().await?;
        let size = validated.file("image").map(|file| file.get_size());
        Response::json(StatusCode::OK, serde_json::json!({ "size": size }))
    })
}
```

### Transfer Encoding {#transfer-encoding}

Use `base64` for canonical Base64 text. For an uploaded file, `encoding:BASE64` checks the PHP mbstring Base64 alphabet, which allows `=`, space, tab, CR, and LF anywhere in the input. The mbstring compatibility check for `Quoted-Printable`, `HTML-ENTITIES` and `UUENCODE` accepts arbitrary bytes.

```rust
use openrust::validation::{Rule, RuleSet, ValidationResult, rules::File};

fn transfer_rules() -> ValidationResult<RuleSet> {
    RuleSet::parse([("value", "required|base64")])?
        .field("upload", [Rule::required(), File::default().encoding("BASE64").into()])
}
```

### UTF-7 and UTF7-IMAP {#utf7-encoding}

Use `UTF-7` for standard UTF-7 or `UTF7-IMAP` for IMAP's modified form. Both check shifted UTF-16 units and reject malformed surrogate pairs and nonzero padding bits. UTF7-IMAP requires a closing `-` for each shifted sequence.

```rust
use openrust::validation::{RuleSet, ValidationResult};

fn mailbox_rules() -> ValidationResult<RuleSet> {
    RuleSet::parse([
        ("subject", "required|string|encoding:UTF-7"),
        ("folder", "required|string|encoding:UTF7-IMAP"),
    ])
}
```

## Validation Error Response Format

For requests that expect JSON, a validation failure returns HTTP 422 with `message` and an `errors` object keyed by field. Expanded keys use dot notation, such as `users.0.email`. Override rule messages and attribute labels in the active locale’s `lang/<locale>/validation.json`.

## Displaying Validation Errors

A failed validation on a session-backed web route redirects to the form and flashes its errors and submitted input for the next request. Blade views receive the request’s error bags as `$errors`. Use `request.old(Some("profile.name"))` to repopulate a field.

The redirect uses the configured target first, then a same-origin Referer, then the previous page stored in the session. The previous page's query string is retained. If no target is available, the response redirects to `/`.

### Named Error Bags

Use `validate_with_bag("profile")` on a validator or override `FormRequest::error_bag` to name the bag. In Blade, `$errors->profile->name` selects the messages for the name field in that bag.

### Excluding Sensitive Input

Password fields are omitted from flashed input. Register additional paths in `with_exceptions` with `dont_flash`.

```rust
use openrust::foundation::configuration::Exceptions;

fn configure(exceptions: &mut Exceptions) {
    exceptions.dont_flash(["profile.secret"]);
}
```
## Standard Rules and Validation Hooks

Size rules calculate numeric values only when `numeric`, `integer`, or `decimal` is declared; otherwise strings use character counts, arrays use item counts, and uploaded files use kilobytes. The error message type follows the declared rules, with numeric before array/list before file before string. Boolean and null values use their scalar text length; floats use the existing 14-significant-digit scalar conversion before size comparisons. Validation preserves the original input type.

The rule parser accepts `numeric`, `date`, `in:a,b`, `regex:/pattern/u`, `required_if:field,value`, `exclude_if:field,value`, `accepted`, `url`, `uuid`, and `array:key1,key2`, alongside the rules described above. `array:key1,key2` rejects unlisted keys; `in` checks each array element when combined with `array`. Excluded fields, including uploaded files, are removed from validated data. Rules run in declaration order.

Default `integer` follows PHP integer validation, including some integral floats and `true`; `integer:strict` requires a JSON integer and `boolean:strict` requires a JSON boolean. Numeric size rules accept negative and decimal bounds and compare decimal values without converting them to binary floats. Scientific-notation exponents outside -1000 to 1000 return an execution error.

Floats first use PHP’s default 14 significant digits when converted to text: the float `1e13` passes, `1e14` fails, and the integer `100000000000000` passes. `regex` accepts only strings or numbers; booleans and `null` without `nullable` fail.

Use `Rule::parse` inside a `RuleSet::field` array for regular expressions containing `|`, so the separator is not interpreted as another rule. PCRE2 modifiers `i m s x u A U J n D S X r` are supported. `D` makes `$` match only the end of the subject unless multiline mode is enabled. `S` is accepted without extra study, and `X` uses PCRE2’s existing strict escape handling. `r` restricts ASCII/non-ASCII case folding. Leading ASCII whitespace is skipped; spaces and newlines between modifiers are ignored. Unknown modifiers, including tabs between modifiers, return `InvalidRule`.

`Validator::after` runs after rule checks and can add errors. `sometimes` conditionally adds rules; wildcard callbacks receive the parent item. `stop_on_first_failure(true)` stops checks across fields, while `bail` stops one field. `safe()` requires successful validation and exposes only validated data.

Form requests execute `prepare_for_validation`, `authorize`, `with_validator`, validation and `after`, then `passed_validation`. Authorization and validation failure stop the action. Precognition with `Precognition-Validate-Only` does not call `passed_validation`; successful Precognition without that header may call it before short-circuiting the action. Each request has its own validator.

## Working with Validated Input {#working-with-validated-input}

`Validator::safe()` returns `support::ValidatedInput` after successful validation; `ValidatedFormRequest::safe()` borrows the already validated result. `only` and `except` select concrete nested paths, `merge` returns a new result, and `collect` returns the existing `Collection<RequestValue>`. The original input remains unchanged. Both borrowed and owned iteration expose top-level key/value pairs.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::RuleSet,
};
use serde_json::json;

fn store_safe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = request.validate(RuleSet::parse([
            ("name", "required|string"),
            ("email", "required|email"),
        ])?).await?;
        let selected = input.only(&["name", "email"])?;
        let without_email = input.except(&["email"])?;
        let merged = input.merge(json!({"role": "reader"}))?;
        let collection = input.collect();
        Response::json(StatusCode::CREATED, json!({
            "selected": selected,
            "without_email": without_email,
            "merged": merged,
            "collection": collection,
            "name": input["name"],
        }))
    })
}
```

Subsets remain typed `ValidatedInput` values rather than untyped arrays so uploaded file handles survive filtering and merging. Use `file()` or typed collection values to access files; JSON projections represent files as `null` without exposing local paths. `to_array()` and indexing expose that JSON projection. `contracts::support::{ValidatedData, Arrayable, ArrayAccess}` provide the corresponding generic contracts. A manually constructed `ValidatedInput::new` does not run validation; use a request or validator for untrusted input.

## Enums and Value Lists {#rule-enum}

`Rule::r#enum::<E>()` and `rules::Enum<E>` validate an application enum through the existing `BackedEnum` mapping. `only` and `except` accept enum cases; a non-empty `only` list takes precedence. `when`, `unless` and `when_else` conditionally configure that same rule. `when_else` takes both branches explicitly because Rust has no optional function arguments. JSON remains a backing scalar, not a Rust enum object.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::BackedEnum,
    validation::{Rule, RuleSet},
};

#[derive(PartialEq)]
enum ServerStatus { Pending, Active }

impl BackedEnum for ServerStatus {
    type Backing = String;
    fn try_from_backing(value: String) -> Option<Self> {
        match value.as_str() {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            _ => None,
        }
    }
}

fn store_status(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rule = Rule::r#enum::<ServerStatus>()
            .when(true, |rule| rule.only([ServerStatus::Active]));
        let data = request.validate(RuleSet::new()
            .field("status", [Rule::required(), rule.into()])?).await?;
        Response::json(StatusCode::CREATED, data)
    })
}

fn store_zone(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = request.validate(RuleSet::new()
            .field("zone", [Rule::required(), Rule::r#in(["first-zone", "second-zone"]).into()])?
            .field("topping", [Rule::not_in(["sprinkles", "cherries"]).into()])?).await?;
        Response::json(StatusCode::CREATED, data)
    })
}
```

`Rule::r#in`, `rules::In`, `Rule::not_in` and `rules::NotIn` keep each list value intact, including commas and quotes. Members accept `Into<serde_json::Value>` and use the existing scalar conversion. Map application enum cases to their backing values explicitly for lists. The `r#` prefix is required for Rust keywords; it does not change the method name. Use `.into()` when combining a typed rule with ordinary rules in one array.

## Fluent Rule Builders {#fluent-rule-builders}

`rules::{StringRule, ArrayRule, Numeric, Date}` compile into the existing rule pipeline; validation order, error messages, wildcard dependencies and validated projection remain shared. Import `RuleBuilder` to chain string and array rules while preserving the existing `Rule::min` and `Rule::max` constructors: Rust cannot declare a static constructor and an inherent instance method with the same name. Typed builders also work directly through `.into()`.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleBuilder, RuleSet},
};

fn store_policy(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("name", [Rule::required(), Rule::string().alpha_dash(true).min(2).max(20)])?
            .field("amount", [Rule::required(), Rule::numeric(false).min(1).max(100).decimal(2, Some(2)).into()])?
            .field("starts_on", [Rule::required(), Rule::date().format("Y-m-d").after("2023-12-31").into()])?
            .field("website", [Rule::required(), Rule::url().protocols(["https"]).into()])?
            .field("id", [Rule::required(), Rule::uuid().version(4).into()])?
            .field("address", [Rule::required(), Rule::ip().version(4)?.into()])?
            .field("zone", [Rule::required(), Rule::timezone(["per_country", "US"]).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}

fn store_typed(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::validation::rules::{ArrayRule, Numeric, StringRule};
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("name", [StringRule::new().min(2).max(20).into()])?
            .field("profile", [ArrayRule::new(["name", "email"]).min(1).into()])?
            .field("count", [Numeric::new(true).integer(true).min(1).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
fn store_condition(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use openrust::validation::RuleCondition;
    Box::pin(async move {
        let required = RuleCondition::callback(|data| Ok(data.as_value()["requires_title"] == true));
        let rules = RuleSet::new()
            .field("title", [Rule::required_if(required).into()])?
            .field("metadata", [Rule::when(true, vec![Rule::array()], Vec::new()).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}

```

`Numeric` supports decimal places, digits, comparisons, multiples and strict integer checks. Fractional bounds use `BigDecimal` explicitly. `Date` supports formats, before/after/equality, exclusive or inclusive ranges, and today/now helpers using the current application clock and timezone. Date arguments accept strings or `support::Date`; date objects are formatted when the constraint is added. Builders support `when`, `unless` and `when_else`.

`rules::{Url, Uuid, IpAddress, Distinct, Timezone}` implement `contracts::validation::ParameterizedRule`; custom implementations can enter through `Rule::parameterized`. `Rule::distinct().strict().ignore_case()` configures element comparisons. IP versions accept 4 or 6 and return an error for other values. URL protocols, UUID versions (including `nil`/`max`) and `numeric:strict` also work in string rules. `integer_strict()` and `boolean_strict()` preserve the existing zero-argument constructors while exposing strict variants.

`Rule::when` and `Rule::unless` choose complete rule lists during compilation, before wildcard expansion. Boolean/callback objects such as `RequiredIf`, `RequiredUnless`, `ExcludeIf`, `ExcludeUnless`, `ProhibitedIf` and `ProhibitedUnless` compile per expanded attribute. `RuleCondition::lazy` accepts a zero-argument callback; `callback` receives the existing typed `Data`. Conditions run again after `set_data` or `set_rules`, not on every validation pass.

### Named Rule Factories {#named-rule-factories}

Named factories use the same validator as string rules. `rules::{AcceptedIf, DeclinedIf, MissingIf, MissingUnless, PresentIf, PresentUnless}` accept a field and a scalar or list of comparison values. `RequiredWith`, `RequiredWithout`, `PresentWith` and related field-list rules take explicit arrays or iterators; Rust has no variadic method arguments. Parameterized rules preserve comma and quote boundaries. `ArrayKeys`, `Contains`, `DoesntContain`, `Regex` and `NotRegex` also work as actual typed rules through `.into()`.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleBuilder, RuleSet},
};
fn store_factories(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("username", [Rule::required(), Rule::regex("/^[a-z]+$/").into(), Rule::confirmed().custom_field("confirmation")])?
            .field("terms", [Rule::accepted_if("account_type", ["personal", "business"]).into()])?
            .field("nickname", [Rule::exclude_with("username").into()])?
            .field("email", [Rule::required_without(["phone", "mobile"]).into()])?
            .field("settings", [Rule::present_if("account_type", "business").into()])?
            .field("profile", [Rule::array_keys(["name", "email"]).into()])?
            .field("roles", [Rule::contains(["reader"]).into(), Rule::doesnt_contain(["admin"]).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
```

`Confirmed::new().custom_field("confirmation")` and `Rule::confirmed().custom_field("confirmation")` select a custom confirmation field. `CurrentPassword::new().guard("web")` or `Rule::current_password(None).guard("web")` selects the named authentication guard, replacing the default selection; this requires a real authenticated guard and configured hasher. `Rule::file()` returns `File`; `Rule::image_file(false)` returns `ImageFile`. ImageFile shares file size, extension, encoding and dimension constraints; pass `true` explicitly to allow SVG.

## Rules that Receive Data and the Validator {#accessing-additional-data}

Implement `DataAwareRule::set_data` to receive the current typed `Data`, including upload metadata. Implement `ValidatorAwareRule::set_validator` to receive a handle to the same validator. Use `Rule::data_aware`, `Rule::validator_aware`, or `Rule::aware` when both traits are implemented. Stable Rust cannot dynamically discover optional traits on a `dyn ValidationRule`, so these adapters explicitly select the setters; `Rule::custom` remains the ordinary stateless entry.

```rust
use openrust::{
    contracts::validation::{DataAwareRule, ValidationRule, ValidatorAwareRule},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{
        Data, Rule, RuleContext, RuleFailure, RuleSet, ValidationError,
        ValidationFuture, ValidationResult, ValidatorHandle,
    },
};
use serde_json::{Value, json};

#[derive(Default)]
struct MatchesConfirmation { expected: Option<Value> }
impl DataAwareRule for MatchesConfirmation {
    fn set_data(&mut self, data: &Data) -> ValidationResult<()> {
        // 只保留当前规则需要的字段，不复制整份输入。
        self.expected = data.as_value().get("confirmation").cloned();
        Ok(())
    }
}
impl ValidationRule for MatchesConfirmation {
    fn validate<'a>(
        &'a self, _: &'a RuleContext<'_>, _: &'a str, value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            Ok(if value == self.expected.as_ref() {
                Vec::new()
            } else {
                vec![RuleFailure::new("validation.confirmed")]
            })
        })
    }
}

#[derive(Default)]
struct NeedsReview { validator: Option<ValidatorHandle> }
impl ValidatorAwareRule for NeedsReview {
    fn set_validator(&mut self, validator: ValidatorHandle) -> ValidationResult<()> {
        self.validator = Some(validator);
        Ok(())
    }
}
impl ValidationRule for NeedsReview {
    fn validate<'a>(
        &'a self, _: &'a RuleContext<'_>, _: &'a str, _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            let validator = self.validator.as_ref().ok_or(ValidationError::NotValidated)?;
            validator.with(|validator| {
                if validator.get_value("review")? == Some(&json!(true)) {
                    validator.errors_mut().add("review", "需要人工复核。");
                }
                Ok(())
            })?;
            Ok(Vec::new())
        })
    }
}

fn store_aware(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("value", [Rule::required(), Rule::data_aware(MatchesConfirmation::default())])?
            .field("status", [Rule::required(), Rule::validator_aware(NeedsReview::default())])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
```

For a rule implementing both traits, `set_data` runs before `set_validator`, followed by `validate`. Normal missing/blank/nullable skipping also skips setters; implicit rules follow their existing scheduling. Setters take `&mut self` and return `ValidationResult<()>` so failures propagate through the existing exception pipeline. Stateful rule instances are serialized only while their setters and validation execute; ordinary rules do not take that lock.

`ValidatorHandle::with` safely reads or mutates the original validator, including values, messages and its error bag. `with_async` also supports an asynchronous borrow. Nested conflicting borrows return an error instead of waiting forever. The handle works only in its current rule task; it becomes invalid after completion or cancellation and is not inherited by spawned tasks. Recursive validation of the same running validator is rejected. `RuleContext` and supplied `Data` are immutable snapshots; use the handle for live state after a mutation. Subsequent rules read the updated value, and cancellation restores the original validator to the caller.

`RuleContext::file()` exposes the uploaded file associated with the original value passed to the rule, even if a setter replaces that field. `context.data` reflects the snapshot taken after the setters; the handle provides the current validator state. Canonical escaped paths stay internal while the rule receives the display attribute name.

## Message Bags {#message-bags}

Use `MessageBag` to collect ordered field errors and `ViewErrorBag` for named bags. `add` suppresses duplicate messages; `merge(&other)` preserves duplicates. `first(key, format)` returns an empty string for a missing message. `get(key, format)` returns `MessageLookup::Exact` or a wildcard mapping that retains matching field names. Use `into_messages()` only when flattening is intended.

```rust
use openrust::support::{MessageBag, ViewErrorBag};

let mut messages = MessageBag::new([("email", ["Invalid email"])]);
messages.add_if(true, "name", "Required");
assert_eq!(messages.first(Some("email"), None), "Invalid email");
assert!(messages.has_any(["email", "phone"]));
messages.set_format(":key: :message");
assert_eq!(messages.first(Some("name"), None), "name: Required");
let mut errors = ViewErrorBag::new();
errors.put("profile", messages);
assert_eq!(errors.get_bag("profile").count(), 2);
assert_eq!(errors.count(), 0);
```

Named bag counts and forwarded read methods use only the `default` bag. Reading a missing bag does not register it; mutate an existing bag with `get_bag_mut`, or add one with `put`. JSON stores raw messages, independent of display format; decoding restores the default format. Message lists use contiguous Rust indices and JSON options follow Serde; PHP array indices, JSON flag bitmasks and runtime method dispatch are not exposed.


## Scalar and digit rules

Use `ascii`, `alpha`, `alpha_dash`, and `alpha_num` for character constraints. The alpha rules accept an `ascii` parameter; their default includes Unicode letters and combining marks, and the latter two also accept numbers. `list` requires contiguous integer keys starting at zero. Use `ip`, `ipv4`, `ipv6`, and `mac_address` for address formats, and `hex_color` for hexadecimal colors prefixed by `#` (3, 4, 6, or 8 digits).

`json` validates scalar JSON text; its parser accepts nesting below 128 containers. `digits:n`, `digits_between:min,max`, `min_digits:n`, and `max_digits:n` count ASCII digits after scalar conversion; signs and decimal points fail these digit rules. `decimal:n` or `decimal:min,max` checks the fractional digit count without accepting scientific notation. Missing required parameters or nonnumeric bounds return `InvalidRule`. These rules follow normal missing-field, blank-string, and `nullable` behavior; add `required` when a value must be provided.

Use `lowercase` and `uppercase` to check Unicode letter case without changing the input. `starts_with:a,b`, `ends_with:a,b`, `doesnt_start_with:a,b`, and `doesnt_end_with:a,b` accept strings and numbers and match any nonempty parameter. `not_regex:/pattern/flags` rejects a match and accepts the same pattern syntax as `regex`. Invalid patterns return `InvalidRule`; use an individual `Rule::parse` in a `RuleSet` when a pattern contains a pipe.

## Presence and Conditional Rules

`missing` requires a field to be absent; a supplied `null` still counts as present. `filled` allows absence but rejects an empty supplied value. `declined` requires `no`, `off`, `0`, integer zero, `false`, or boolean false. `prohibited` allows absence and empty values and rejects a populated value. `same:other` uses strict equality; `different:other,third` checks each supplied peer with strict equality.

Use `accepted_if`, `declined_if`, `missing_if`, `missing_unless`, `missing_with`, `missing_with_all`, `present_if`, `present_unless`, `present_with`, and `present_with_all` for conditional presence. Required values support `required_unless`, `required_with`, `required_with_all`, `required_without`, `required_without_all`, `required_if_accepted`, and `required_if_declined`. Presence checks count null as present; required checks also consider empty strings and arrays. Conditional parameters resolve wildcard indices from the current attribute.

`prohibited_if`, `prohibited_unless`, `prohibited_if_accepted`, `prohibited_if_declined`, and `prohibits` reject populated fields under the named condition. `exclude`, `exclude_unless`, `exclude_with`, and `exclude_without` remove matching fields from validated data, along with their child errors and uploaded files. Exclusion participates in the declared rule order; a preceding required failure can stop evaluation before exclusion. `nullable` does not bypass implicit presence rules.

## Array Membership and Formats

`array_keys:a,b` limits allowed keys; `required_array_keys:a,b` requires every key, and `in_array_keys:a,b` requires at least one. `contains:a,b` uses loose value comparison for every requested value; `doesnt_contain:a,b` uses strict comparison to reject each listed string. `not_in:a,b` reverses `in`; with an `array` rule, it reverses the result for the whole array.

`in_array:pool.*` checks strict equality against flattened candidate fields. `distinct` rejects duplicates under the declared wildcard field; `strict` preserves scalar types, while `ignore_case` uses Unicode regular-expression case matching for strings. `date_format:Y-m-d,Y/m/d` accepts any supplied format and checks the formatted result against the original input; its error names the first format. `ulid` checks a 26-character ULID, including the first-character bound.

`multiple_of:0.1` checks an exact decimal remainder. A zero divisor always fails. Scientific exponents outside -1000 to 1000 return an execution error, including in the divisor. Customize array-key errors with `:unexpected`; `:Unexpected` capitalizes the whole list only at its start, while `:UNEXPECTED` capitalizes every letter.

## Dates, Sizes, and DNS

`before:other`, `before_or_equal:other`, `after:other`, `after_or_equal:other`, and `date_equals:other` compare dates against a literal or another field. They use the application timezone and test clock. Without `date_format`, comparisons use whole seconds; with a format, microseconds participate. Use `date` or `date_format` alongside a comparison when the input itself must be a valid date.

`gt:other`, `lt:other`, `gte:other`, and `lte:other` compare numeric values, string lengths, array counts, or file kilobytes. Numeric inputs support a literal threshold. Peer values must have the same type unless both are numeric. Each comparison determines its own numeric classification; it does not change later size rules.

`timezone` accepts exact identifiers from the bundled IANA 2025b data. Use `timezone:all_with_bc` to include aliases, a region such as `timezone:asia`, or `timezone:per_country,CN` for a country. Names are case sensitive; the group and country parameters are case insensitive. Invalid groups or country parameter lengths return `InvalidRule`.

`active_url` requires a host with DNS A or AAAA records. It uses asynchronous DNS queries and does not accept a numeric IP merely because the operating system can resolve it. `validator.set_dns_resolver(resolver)` accepts a Hickory 0.25 resolver with explicit nameservers, timeout and retry settings. DNS lookup failures fail validation. `validator.fake_dns_lookups(true)` enables hostname-only test behavior for that validator; pass `false` to restore real DNS. The test switch is instance scoped so concurrent validators remain independent.

## Updating a Validator

Use `get_data`, `get_value`, and `get_rules` to inspect input and expanded rules. `get_rules` returns ordered `(attribute, rules)` pairs using native escaped paths. `set_value` updates one concrete path; wildcards are rejected. `set_data` replaces the input and restores the rules supplied to `set_rules` or the factory. `add_rules` merges temporary rules; `append_rules` also makes the merged declarations the next reset baseline.

`set_custom_messages` merges messages. `set_attribute_names` and `set_value_names` replace their maps, while `add_custom_attributes` and `add_custom_values` merge at the field level. `get_translator` and `set_translator` expose the translator used for errors. Outside an active validation pass, mutations discard cached success and errors; call `validate().await` again before reading `validated()` or `safe()`.

`get_exception` and `set_exception` use a typed function `fn(MessageBag, Option<&Translator>) -> ValidationException`. It can customize the actual rejection status, error bag, and redirect. The return type replaces runtime exception class names and inheritance; each validator keeps its own factory.

## Registering validation extensions

In a service provider, use `Validator::extend` for ordinary rules, `extend_implicit` for rules that must also inspect missing or empty values, and `extend_dependent` for rules whose parameters reference other fields. Register `ProjectCode` or `project_code` and use `project_code` in declarations. Each validator receives a snapshot of the factory registrations when created.

```rust
use openrust::support::facades::Validator;

Validator::extend("upper_code", |_, value, _, _| {
    Ok(value.and_then(serde_json::Value::as_str)
        .is_some_and(|value| value.chars().all(|ch| ch.is_ascii_uppercase())))
}, Some("The :attribute must contain uppercase letters."))?;
```

Callbacks receive the attribute, `Option<&Value>`, parameters and a shared validator reference, and return `ValidationResult<bool>`. Ordinary rules skip missing values and empty strings; `nullable` skips null, and `sometimes` still skips missing fields. Implicit rules can inspect missing, empty and null values. Dependent rules expand parameters such as `items.*.other` for the current item. An `Err` propagates as an execution error. Use `Rule::custom` and `ValidationRule` for async rules.

Register a message callback using `Validator::replacer`. It receives the selected message, attribute, rule name, parameter list and shared validator reference, and returns the final message. Custom or translated messages and attribute names are resolved first. A registered replacer takes responsibility for rule placeholders such as `:min`. `Factory::resolver` accepts and returns an owned validator for construction customization. Rust uses closures and explicitly resolved services; runtime `Class@method` strings and dynamic validator subclasses are unsupported.

On one validator, use `add_extension`, `add_implicit_extension`, `add_dependent_extension` and `add_replacer`. Their plural forms accept `(String, Arc<callback>)` entries. These changes invalidate cached validation results and require revalidation.

The default factory policy is `exclude_unvalidated_array_keys()`: when a parent has an unparameterized `array` or `list` rule and nested rules, validated output contains only the validated child keys. `include_unvalidated_array_keys()` includes all parent keys for subsequently created validators; `exclude_unvalidated_array_keys()` restores the default. `array:name,email` already restricts allowed keys and does not use this pruning policy. The public `exclude_unvalidated_array_keys` boolean can be changed on one validator; subsequent `validated()` and `safe()` calls use that policy immediately.

## Custom messages and field labels {#custom-validation-messages}

Use `validate_with(rules, messages, attributes)` for the optional message and attribute maps. Use `validate_with_bag_and_options(bag, rules, messages, attributes)` for a named bag. The Request facade exposes the same entries; existing `validate` and `validate_with_bag` calls keep their defaults. Rust represents PHP’s optional arguments with explicit additional methods.

```rust
use std::collections::BTreeMap;
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::RuleSet,
};

fn store_with_messages(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::parse([("title", "required|string|min:3")])?;
        let messages = BTreeMap::from([("title.required".into(), "Please provide :attribute.".into())]);
        let attributes = BTreeMap::from([("title".into(), "article title".into())]);
        let data = request.validate_with(rules, messages, attributes).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

For manually constructed validators, call `Validator::make_with(data, rules, messages, attributes)`. A Form Request can override `messages()` and `attributes()`, both returning `BTreeMap<String, String>`. All entries use the same application factory and preserve JSON 422, Web redirects/error bags and Precognition. These maps apply only to that validator and do not change shared translations.

## Rejecting unknown fields {#failing-on-unknown-fields}

Return `Some(true)` from `FormRequest::fail_on_unknown_fields` to reject body fields absent from the declared rules. `Some(false)` explicitly disables it for that form; the default `None` inherits the application default. The default application policy is disabled.

```rust
use openrust::{
    foundation::http::FormRequest,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{RuleSet, ValidationResult},
};

struct StoreArticle;

impl FormRequest for StoreArticle {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("title", "required|string")])
    }

    fn fail_on_unknown_fields(&self) -> Option<bool> {
        Some(true)
    }
}

fn store_strict(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = StoreArticle.validate_resolved(request).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

In a provider’s boot method, resolve `foundation::http::FormRequestDefaults` through `app.container().make::<FormRequestDefaults>()?` and call `fail_on_unknown_fields(true)`. This replaces the upstream process-wide static toggle with application-scoped state, preventing one application from changing another. Validation uses body leaf keys, including nested and wildcard declarations and confirmation companions. Query parameters, uploads, `_token` and `_method` are not rejected by this policy. An `array` rule alone does not authorize every child key. Failures use the existing translated `validation.prohibited` message, JSON 422 or Web error bag; Precognition applies this check before successful preflight completion.

A named bag also works with `@error` and its `@else` fallback:

```blade
@error('title', 'article')
    <span>{{ $message }}</span>
@else
    <span>—</span>
@enderror
```

## Generate rule objects {#creating-custom-rules}

Create a normal rule or an implicit rule with the following commands (use separate names):

```sh
./artisan make:rule Uppercase
./artisan make:rule RequiredTitle --implicit
```

Files are written to `app/rules/uppercase.rs` and `app/rules/required_title.rs`. Declare them in `app/rules/mod.rs` and expose the `rules` module from `app/mod.rs`, then import the generated type and pass it to `Rule::custom`. Nested names such as `Admin/Uppercase` are supported. `-i` is the short form of `--implicit`; `-f` / `--force` overwrites an existing file. Without force, a duplicate reports a conflict and preserves the original file.

`./artisan stub:publish` exposes `stubs/rule.rs.stub` and `stubs/rule.implicit.rs.stub` for application-specific generation. Both templates initially return an empty failure list: an implicit flag alone does not require a field. Implement rejection in `validate`, for example as shown below. Module-qualified imports also allow type names such as `RuleContext`, `Box`, `Option` or `Result` without conflicting with template internals.

## Field-specific closures {#using-closures}

Pass a synchronous closure directly to `Rule::custom`; no application-wide registration is needed. It receives the current rule context, the unescaped attribute name and `Option<&Value>`; return a list of `RuleFailure` values. An execution error propagates through the normal HTTP error pipeline. Rule objects keep the existing async `ValidationRule::validate` contract.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleContext, RuleFailure, RuleSet, ValidationResult},
};
use serde_json::Value;

fn store_callback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rule = Rule::custom(|_: &RuleContext<'_>, attribute: &str, value: Option<&Value>| -> ValidationResult<_> {
            Ok(if value.and_then(Value::as_str) == Some("foo") {
                vec![RuleFailure::new(format!("The {attribute} is invalid."))]
            } else {
                Vec::new()
            })
        });
        let rules = RuleSet::new().field("title", [Rule::required(), Rule::string(), rule])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

## Implicit rule objects {#implicit-rules}

Override `ValidationRule::is_implicit()` to run when a field is missing, empty or null, including nullable fields. `sometimes` still skips an absent field. The flag does not reject input by itself: return a failure when required. Ordinary callbacks skip absent fields, raw empty strings and nullable nulls; normal HTTP empty-string-to-null processing occurs before request validation. `RuleContext::data` exposes the input used by this validator without copying it into shared rule state. Failure translation keys use the validator’s current translator; ValidatorAware setters are not provided by this interface.

```rust
use openrust::{
    contracts::validation::ValidationRule,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleContext, RuleFailure, RuleSet, ValidationFuture, ValidationResult},
};
use serde_json::Value;

struct RequiredTitle;

impl ValidationRule for RequiredTitle {
    fn is_implicit(&self) -> bool { true }

    fn validate<'a>(
        &'a self,
        _context: &'a RuleContext<'_>,
        _attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            Ok(if value.is_none_or(|v| v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty())) {
                vec![RuleFailure::new("validation.required")]
            } else {
                Vec::new()
            })
        })
    }
}

fn store_implicit(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field("title", [Rule::bail(), Rule::custom(RequiredTitle), Rule::string()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

## Translate rule failures {#translating-validation-messages}

Call `RuleFailure::translate` with the current `RuleContext`, replacement values and an optional locale; `None` uses the current validator language. Use application keys such as `rules.invalid` from `lang/fr/rules.json`. `translate_choice(context, count, replacements, locale)` additionally selects a plural branch using the existing translator. Each failure keeps its own selection: it does not change another failure or the request language. Missing keys remain keys, and literal messages still receive replacements. Explicitly translated messages bypass generic rule-message overrides. Construct failures through `RuleFailure::new`; the translation marker is not serialized into HTTP responses. The explicit context borrows the original translator and resolves the message immediately; it avoids hidden global state and delayed locale selection.

```rust
use openrust::validation::{RuleContext, RuleFailure};
use std::collections::BTreeMap;

fn localized_failure(context: &RuleContext<'_>) -> RuleFailure {
    RuleFailure::new("rules.invalid").translate(
        context,
        BTreeMap::from([("token".into(), "X".into())]),
        Some("fr"),
    )
}
```

## Any complete rule group {#rule-any-of}

`Rule::any_of` accepts alternative lists of rules. Every rule in one group must pass; later groups stop running after a successful group. `rules::AnyOf::new` provides the same builder. Add `required` separately when an absent field must be rejected.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleSet},
};

fn store_choice(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let choice = Rule::any_of([
            vec![Rule::email()],
            vec![Rule::string(), Rule::min(6)],
        ]);
        let rules = RuleSet::new().field("contact", [Rule::required(), choice.into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}

fn store_contact(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let choice = Rule::any_of([
            RuleSet::parse([("email", "required|email")])?,
            RuleSet::parse([("name", "required|string|min:6")])?,
        ]);
        let rules = RuleSet::new().field("contact", [Rule::required(), choice.into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

For associative input, pass alternatives built with `RuleSet`; each group checks its own field mapping. Child validators use the current application factory, including extensions, database presence checks, custom messages and attribute names. Uploaded files keep their typed handles in nested groups. A group check does not replace the outer value with the child’s filtered result: use allowed array keys when additional fields must be rejected.

This rule requires an active application facade scope, like the other validation facades. Empty alternatives reject a supplied value; an empty individual group passes. All-failed groups report `validation.any_of` on the outer field and support its custom message.

## Compiling per-item rules {#accessing-nested-array-data}

`Rule::for_each` returns `NestedRules`. The callback runs when the validator is created, before any rules are evaluated, and receives the current value and expanded attribute. Return `Vec<Rule>` for the current value or a `RuleSet` whose fields are relative to it. Replacing all data or rules recompiles callbacks; repeated validation does not rerun them.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{Rule, RuleSet},
};
use serde_json::Value;

fn store_each(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("companies", [Rule::array()])?
            .field("companies.*.id", [Rule::for_each(|_value, _attribute| {
                Ok(vec![Rule::required(), Rule::integer(), Rule::min(1)])
            }).into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}

fn store_bulk(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("items", [Rule::array()])?
            .field("items.*.bulk", [Rule::sometimes(), Rule::parse("boolean:strict")?])?
            .field("items.*.quantity", [Rule::for_each_with(|_value, _attribute, _flat, item| {
                let bulk = item.and_then(|item| item.get("bulk")).and_then(Value::as_bool) == Some(true);
                Ok(vec![Rule::required(), Rule::integer(), Rule::min(if bulk { 10 } else { 1 })])
            }).into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}

fn store_nested(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new()
            .field("items", [Rule::array()])?
            .field("items.*", [Rule::for_each(|_value, _attribute| {
                RuleSet::parse([("id", "required|integer"), ("tag", "sometimes|string")])
            }).into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::CREATED, data.into_value())
    })
}
```

Use `Rule::for_each_with` or `NestedRules::new` for all four arguments: value, attribute, gathered flat data and the parent context. The flat map uses canonical escaped paths so literal dots and nested paths remain distinct. `contracts::validation::CompilableRules` and `Rule::compile` are available for custom compilers. Database rules inside the returned list use the existing factory and presence verifier. For file-dependent compilation, use `Rule::for_each_context` or `NestedRules::with_context`: `NestedRuleContext::file()` reads the original typed file, `data` holds the original input, and `flattened` holds the current gathered map. Its `attribute` is a canonical escaped path. `CompilableRules::compile_context` exposes the same context to custom compilers.

Compiled fields retain wildcard message lookup, error positions, Precognition selection and validated-field filtering. Existing infallible `set_data` / `set_rules` setters retain compilation errors; the next `get_rules` or validation returns the original error kind. Corrected data or rules clears the error. Empty typed maps remain JSON objects; no PHP array/object ambiguity is introduced into existing response contracts.

## Array Error Indexes and Positions {#error-message-indexes-and-positions}

`:index` starts at zero, `:position` at one, and `:ordinal-position` renders an ordinal such as `2nd`. Use `:second-index`, `:second-position`, `:second-ordinal-position` and corresponding third through tenth forms for deeper numeric path segments; `other` refers to the eleventh numeric segment. Paths without that level leave its placeholder unchanged. `:input` uses the display value for scalar or null input, including configured value labels; arrays remain unreplaced. Ordinals reuse number formatting with its default English locale, independently of the validation message language.

```rust
use std::collections::BTreeMap;
use openrust::{http::{HttpError, HttpFuture, Request, Response, StatusCode}, validation::RuleSet};
fn store_photos(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::parse([("photos.*.description", "required|string")])?;
        let messages = BTreeMap::from([("photos.*.description.required".into(), "Please describe photo #:position (:ordinal-position), input=:input.".into())]);
        let data = request.validate_with(rules, messages, BTreeMap::new()).await?;
        Response::json(StatusCode::CREATED, data)
    })
}
```

## Email Validation Modes {#email-validation-modes}

`RuleBuilder` preserves the existing `Rule::email()` return type while adding the fluent methods. `Email::new()` provides the typed builder, including `strict`, `when`, `unless`, `when_else` and additional `rules`. Additional checks use the original factory, including its resolver and current extension registrations, to create a nested validator with the original typed data, messages and attributes; custom DataAware/ValidatorAware rules receive that nested validator.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Validator,
    validation::{Rule, RuleBuilder, RuleSet, ValidationResult, rules::Email},
};
fn store_email(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field("email", [Rule::required(), Rule::email()
            .rfc_compliant(false).validate_mx_record().prevent_spoofing()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
fn store_native_email(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field("email", [Email::new().with_native_validation(true).into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
fn store_custom_email(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let email = Email::new().rfc_compliant(false)
            .rules([Rule::not_in(["blocked@example.com"]).into()]);
        let rules = RuleSet::new().field("email", [email.into()])?;
        Response::json(StatusCode::CREATED, request.validate(rules).await?)
    })
}
fn configure_email_tests() -> ValidationResult<()> {
    Validator::fake_dns_lookups(true)
}
```

In tests, call `Validator::fake_dns_lookups(true)` inside the application scope, or configure the application’s `Factory::fake_dns_lookups(true)`. Pass `false` to restore real lookups. Already-created validators observe their application’s factory switch, while other applications stay independent. The instance method remains an explicit per-validator override. Rust requires the boolean argument. DNS faking preserves the other selected checks and reserved-domain handling; use a host such as `example.com` instead of `.test` for a successful fake.

Factory clones share registrations and the DNS test policy. Create a new factory with `Factory::default` or `Factory::new` when independent registration state is required.

## Language file messages and labels {#language-file-messages}

Create or override keys in `lang/en/validation.json`. Language-file `custom` wildcards can span nested paths; inline messages and attribute-name wildcards match one path segment. Size-specific custom messages select `numeric`, `array`, `file` or `string` using the declared rules. Exact entries take priority. Labels are looked up in the current language’s complete attributes group before group-level fallback. Unlabelled wildcard fields keep their raw path; ordinary field names are converted from snake/camel case to words.

```json
{
  "custom": {
    "photos.*.description": {
      "min": {
        "string": "Photo :position: :attribute needs :min characters."
      }
    }
  },
  "attributes": {
    "photos.*.description": "description"
  }
}
```

## Password policies {#password-policies}

Register the provider in `bootstrap/providers.rs`. `Password::defaults()` reads the application policy; `set_defaults` registers its callback. Rust separates the zero-argument getter and callback setter instead of overloading one dynamic method. Without an application context, the default requires eight characters. `required()` and `sometimes()` use the configured policy, while `min(n)` starts a fresh explicit policy. Policies already constructed keep their values when defaults change.

```rust
use openrust::{
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{ProviderResult, ServiceProvider},
    validation::{Rule, RuleSet, rules::Password},
};

struct PasswordServiceProvider;
impl ServiceProvider for PasswordServiceProvider {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            Password::set_defaults(|| Password::min(12).mixed_case().numbers())?;
            Ok(())
        })
    }
}

fn store_password(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rule = Password::defaults()?.symbols().rules([Rule::max(64)]);
        let data = request.validate(RuleSet::new().field("password", [rule.into()])?).await?;
        Response::json(StatusCode::CREATED, data)
    })
}
```

Use `letters`, `mixed_case`, `numbers`, `symbols`, `max`, `when`, `unless` and `when_else` to compose a policy. `rules` replaces the previous additional rule list. Base and additional checks use the original validator factory, including custom messages and aware rules; complexity checks still inspect the original password. The policy is iterable as its base/additional rules; complexity and breach checks belong to the policy itself. `applied_rules()` returns typed settings, and `to_password_rules_string()` builds an HTML `passwordrules` attribute.

Add `.uncompromised(0)` to reject any known occurrence, or pass a higher threshold to allow that many occurrences. The default verifier sends only the first five uppercase SHA-1 characters to the breach range API with padding enabled. Basic failures do not query it. As in the fixed mechanism, connection exceptions are reported and an unavailable service or unsuccessful response permits the password; this check supplements the local policy. The default timeout is 30 seconds. Override the `UncompromisedVerifier` contract for another source; standalone factories can use `set_uncompromised_verifier`. Trait arguments are an explicit value and signed threshold, not a dynamic data array. Application-local defaults and bindings isolate concurrent applications; they do not use process-global mutable state.

## Direct validation and manual error redirects {#direct-validator-entries}

`Validator::validate` creates and runs the configured application validator. Use `validate_with` for custom messages and labels; `Factory::validate` accepts all four explicit arguments. The concrete validator’s `passes().await` reruns validation and returns the opposite of `fails()`.

```rust
use openrust::{
    contracts::validation::Validator as _,
    foundation::http::FormRequest,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Redirect, Validator},
    validation::{AfterValidation, AfterValidationRule, Data, RuleSet, ValidationResult},
};

fn validate_article(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Validator::validate(
            Data::from_request(request).await?,
            RuleSet::parse([("title", "required|string|min:3")])?,
        ).await?;
        Response::json(StatusCode::CREATED, data)
    })
}

fn save_form(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut validator = Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([("title", "required|string|min:3")])?,
        )?;
        if !validator.passes().await? {
            return Ok(Redirect::to("/edit", Default::default())?
                .with_errors(request, &validator, "article")?
                .with_input(request, None).await?.into());
        }
        Response::json(StatusCode::CREATED, validator.validated()?)
    })
}

struct CheckTitle;
impl AfterValidationRule for CheckTitle {
    fn after(&self, validator: &mut openrust::validation::Validator) -> ValidationResult<()> {
        if validator.get_value("title")?.and_then(|value| value.as_str()) == Some("blocked") {
            validator.errors_mut().add("title", "Title is blocked.");
        }
        Ok(())
    }
}

struct StoreArticle;
impl FormRequest for StoreArticle {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("title", "required|string")])
    }
    fn after(&self) -> Vec<AfterValidation> {
        vec![CheckTitle.into_callback()]
    }
}
fn validate_form(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = StoreArticle.validate_resolved(request).await?;
        Response::json(StatusCode::CREATED, data)
    })
}
```

`with_errors` accepts a validator, a `MessageProvider`, a message bag, or message arrays such as `[("title", "Invalid title.")]`. Existing bags are borrowed until session storage; the adapter does not clone them first. Specify the error bag and request explicitly. `after_many` adds a list of boxed callbacks; `after_rule` and `after_rules` register services implementing `AfterValidationRule`. In a Form Request, `into_callback()` converts that service into the existing callback type while preserving its place in the list. These methods adapt dynamic callable forms without changing callback order or the validation pipeline.

## Conditional rules for several fields {#conditional-multiple-fields}

`sometimes` accepts one field, an array, a vector, or a borrowed slice of fields. It keeps field order, repeated fields, and each wildcard item’s callback context. Conditions read the original data before validation, so this example reuses the scalar comparison bridge to preserve numeric-string and boolean behavior. Rust callbacks receive `&Data` and `Option<&Value>` instead of a dynamic Fluent object. Single-field borrowing does not allocate a new string.

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{facades::Validator, php_integer_less_than_value, php_loose_string_value_equal},
    validation::{Data, Rule, RuleSet},
};

pub fn store_collector(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut validator = Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([("games", "required|integer|min:0")])?,
        )?;
        validator.sometimes(["reason", "cost"], vec![Rule::required()], |input, _| {
            let games = &input.as_value()["games"];
            php_integer_less_than_value(100, games) || php_loose_string_value_equal("100", games)
        })?;
        Response::json(StatusCode::CREATED, validator.validate_request(request).await?)
    })
}
```

Inline messages search attribute/rule, rule, and attribute in that order, retaining the native `BTreeMap` order within each tier. A wildcard can win before a later exact entry in that tier. Avoid overlapping entries when order should not decide the message. Inline wildcards match one segment, including wildcard rule names; language-file custom messages use their separate cross-path lookup. A false `sometimes` condition still records the parsed wildcard’s primary name, as required for later error labels.

## Displaying errors and repopulating a form {#displaying-errors-and-old-input}

The `web` middleware shares a request-local `$errors` object, including when there are no errors. Use the default bag’s `any`, `all`, `first`, `get` and `has` methods, or select a named bag. `@error` reads the first matching message and supports wildcard fields; `$message` is local to the directive.

```blade
@if ($errors->any())
    <ul>
        @foreach ($errors->all() as $error)
            <li>{{ $error }}</li>
        @endforeach
    </ul>
@endif

<input name="title" value="{{ old('title', 'Untitled') }}">
@error('title')
    <p>{{ $message }}</p>
@enderror

@error('title', 'login')
    <p>{{ $message }}</p>
@enderror
<p>{{ $errors->login->first('title') }}</p>
```

`old(key, default)` and `request()->old(key, default)` read the existing session flash through the HTTP rendering bridge. A missing key uses the default; a stored `null` remains null. Render with a returned view or `Response::view` so the actual request is available. Escaped echoes protect both error text and old values. The next request ages out the flash normally; cached templates receive the current request’s errors and old input.


## Form Request redirect targets {#form-request-redirect-targets}

Override `redirect_to()` for a URL, or `redirect_route()` for a named route. An explicit URL takes priority and is generated through the current request’s URL generator; the named route is resolved only after a validation failure. This corresponds to the URL and named-route attributes using native trait methods. Register the `dashboard` route before accepting requests; an unknown name propagates its URL-generation error instead of silently falling back. JSON failures still return 422 when the target resolves; successful validation does not resolve the target.

```rust
use openrust::{
    foundation::http::{FormRequest, ValidatedFormRequest},
    http::{HttpError, Response, StatusCode},
    validation::{RuleSet, ValidationResult},
};

#[derive(Default)]
pub struct StorePostRequest;

impl FormRequest for StorePostRequest {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("title", "required|string")])
    }

    fn redirect_route(&self) -> Option<&str> {
        Some("dashboard")
    }

    fn error_bag(&self) -> &str {
        "post"
    }
}

pub async fn store_post(_: ValidatedFormRequest<StorePostRequest>) -> Result<Response, HttpError> {
    Ok(Response::empty(StatusCode::CREATED))
}
```
