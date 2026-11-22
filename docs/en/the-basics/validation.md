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
                .withoutTrashed(None)
                .into(),
        ])
}
```

Fluent rules support query constraints and soft-deleted rows. When updating a row, use `ignore` with a trusted identifier or `ignoreModel` with a model already resolved and authorized by the application. Use a database unique constraint to enforce uniqueness across concurrent writes.

## Form Request Validation {#form-request-validation}

Use `ValidatedFormRequest<F>` as a typed action parameter. The router creates the form request, prepares the input, authorizes, validates, and calls `passedValidation` before dispatching the action. An authorization denial returns HTTP 403.

```rust
use openrust::{
    foundation::http::{FormRequest, ValidatedFormRequest},
    http::{HttpError, Method, Response, StatusCode},
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
    Route::r#match(&[Method::POST], "/users", store)
}
```

A form request implements `Default` for construction. A transient container binding for the form type can supply constructor dependencies. Use `ControllerDispatcher::withFormRequest` for a controller method or `ResourceController::actionWithFormRequest` for a resource action. A controller using either adapter also receives the original request and its route parameters. Call `validateResolved` for explicit validation.

## Validating Files {#validating-files}

Use `Data::fromRequest` for manual multipart validation. Form requests include uploads automatically. Access validated uploads by dotted path with `validated.file`, including indexed paths such as `attachments.0`. The JSON view represents file leaves as null.

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
        let data = Data::fromRequest(request).await?;
        let rules = RuleSet::new().field("image", [
            Rule::required(),
            File::image(false)
                .dimensions(Rule::dimensions().minWidth(256).ratio("1/1"))
                .max("2mb")
                .into(),
        ])?;
        let validated = Validator::make(data, rules)?.validate().await?;
        let size = validated.file("image").map(|file| file.getSize());
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

Use `validateWithBag("profile")` on a validator or override `FormRequest::errorBag` to name the bag. In Blade, `$errors->profile->name` selects the messages for the name field in that bag.

### Excluding Sensitive Input

Password fields are omitted from flashed input. Register additional paths in `withExceptions` with `dontFlash`.

```rust
use openrust::foundation::configuration::Exceptions;

fn configure(exceptions: &mut Exceptions) {
    exceptions.dontFlash(["profile.secret"]);
}
```
## Standard Rules and Validation Hooks

The rule parser accepts `numeric`, `date`, `in:a,b`, `regex:/pattern/u`, `required_if:field,value`, `exclude_if:field,value`, `accepted`, `url`, `uuid`, and `array:key1,key2`, alongside the rules described above. `array:key1,key2` rejects unlisted keys; `in` checks each array element when combined with `array`. Excluded fields, including uploaded files, are removed from validated data. Rules run in declaration order.

Default `integer` follows PHP integer validation, including some integral floats and `true`; `integer:strict` requires a JSON integer and `boolean:strict` requires a JSON boolean. Numeric size rules accept negative and decimal bounds and compare decimal values without converting them to binary floats. Scientific-notation exponents outside -1000 to 1000 return an execution error.

Floats first use PHP’s default 14 significant digits when converted to text: the float `1e13` passes, `1e14` fails, and the integer `100000000000000` passes. `regex` accepts only strings or numbers; booleans and `null` without `nullable` fail.

Use `Rule::parse` inside a `RuleSet::field` array for regular expressions containing `|`, so the separator is not interpreted as another rule. PCRE2 modifiers `i m s x u A U J n D` are supported. `D` makes `$` match only the end of the subject unless multiline mode is enabled. Other unsupported modifiers return `InvalidRule`; the parser does not claim every PHP PCRE option.

`Validator::after` runs after rule checks and can add errors. `sometimes` conditionally adds rules; wildcard callbacks receive the parent item. `stopOnFirstFailure(true)` stops checks across fields, while `bail` stops one field. `safe()` requires successful validation and exposes only validated data.

Form requests execute `prepareForValidation`, `authorize`, `withValidator`, validation and `after`, then `passedValidation`. Authorization and validation failure stop the action. Precognition with `Precognition-Validate-Only` does not call `passedValidation`; successful Precognition without that header may call it before short-circuiting the action. Each request has its own validator.

## Message Bags {#message-bags}

Use `MessageBag` to collect ordered field errors and `ViewErrorBag` for named bags. `add` suppresses duplicate messages; `merge(&other)` preserves duplicates. `first(key, format)` returns an empty string for a missing message. `get(key, format)` returns `MessageLookup::Exact` or a wildcard mapping that retains matching field names. Use `intoMessages()` only when flattening is intended.

```rust
use openrust::support::{MessageBag, ViewErrorBag};

let mut messages = MessageBag::new([("email", ["Invalid email"])]);
messages.addIf(true, "name", "Required");
assert_eq!(messages.first(Some("email"), None), "Invalid email");
assert!(messages.hasAny(["email", "phone"]));
messages.setFormat(":key: :message");
assert_eq!(messages.first(Some("name"), None), "name: Required");
let mut errors = ViewErrorBag::new();
errors.put("profile", messages);
assert_eq!(errors.getBag("profile").count(), 2);
assert_eq!(errors.count(), 0);
```

Named bag counts and forwarded read methods use only the `default` bag. Reading a missing bag does not register it; mutate an existing bag with `getBagMut`, or add one with `put`. JSON stores raw messages, independent of display format; decoding restores the default format. Message lists use contiguous Rust indices and JSON options follow Serde; PHP array indices, JSON flag bitmasks and runtime method dispatch are not exposed.


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

`active_url` requires a host with DNS A or AAAA records. It uses asynchronous DNS queries and does not accept a numeric IP merely because the operating system can resolve it. `validator.setDnsResolver(resolver)` accepts a Hickory 0.25 resolver with explicit nameservers, timeout and retry settings. DNS lookup failures fail validation. `validator.fakeDnsLookups(true)` enables hostname-only test behavior for that validator; pass `false` to restore real DNS. The test switch is instance scoped so concurrent validators remain independent.

## Updating a Validator

Use `getData`, `getValue`, and `getRules` to inspect input and expanded rules. `getRules` returns ordered `(attribute, rules)` pairs using native escaped paths. `setValue` updates one concrete path; wildcards are rejected. `setData` replaces the input and restores the rules supplied to `setRules` or the factory. `addRules` merges temporary rules; `appendRules` also makes the merged declarations the next reset baseline.

`setCustomMessages` merges messages. `setAttributeNames` and `setValueNames` replace their maps, while `addCustomAttributes` and `addCustomValues` merge at the field level. `getTranslator` and `setTranslator` expose the translator used for errors. Mutations discard cached success and errors; call `validate().await` again before reading `validated()` or `safe()`.

`getException` and `setException` use a typed function `fn(MessageBag, Option<&Translator>) -> ValidationException`. It can customize the actual rejection status, error bag, and redirect. The return type replaces runtime exception class names and inheritance; each validator keeps its own factory.

## Registering validation extensions

In a service provider, use `Validator::extend` for ordinary rules, `extendImplicit` for rules that must also inspect missing or empty values, and `extendDependent` for rules whose parameters reference other fields. Register `ProjectCode` or `project_code` and use `project_code` in declarations. Each validator receives a snapshot of the factory registrations when created.

```rust
use openrust::support::facades::Validator;

Validator::extend("upper_code", |_, value, _, _| {
    Ok(value.and_then(serde_json::Value::as_str)
        .is_some_and(|value| value.chars().all(|ch| ch.is_ascii_uppercase())))
}, Some("The :attribute must contain uppercase letters."))?;
```

Callbacks receive the attribute, `Option<&Value>`, parameters and a shared validator reference, and return `ValidationResult<bool>`. Ordinary rules skip missing values and empty strings; `nullable` skips null, and `sometimes` still skips missing fields. Implicit rules can inspect missing, empty and null values. Dependent rules expand parameters such as `items.*.other` for the current item. An `Err` propagates as an execution error. Use `Rule::custom` and `ValidationRule` for async rules.

Register a message callback using `Validator::replacer`. It receives the selected message, attribute, rule name, parameter list and shared validator reference, and returns the final message. Custom or translated messages and attribute names are resolved first. A registered replacer takes responsibility for rule placeholders such as `:min`. `Factory::resolver` accepts and returns an owned validator for construction customization. Rust uses closures and explicitly resolved services; runtime `Class@method` strings and dynamic validator subclasses are unsupported.

On one validator, use `addExtension`, `addImplicitExtension`, `addDependentExtension` and `addReplacer`. Their plural forms accept `(String, Arc<callback>)` entries. These changes invalidate cached validation results and require revalidation.

The default factory policy is `excludeUnvalidatedArrayKeys()`: when a parent has an unparameterized `array` or `list` rule and nested rules, validated output contains only the validated child keys. `includeUnvalidatedArrayKeys()` includes all parent keys for subsequently created validators; `excludeUnvalidatedArrayKeys()` restores the default. `array:name,email` already restricts allowed keys and does not use this pruning policy. The public `excludeUnvalidatedArrayKeys` boolean can be changed on one validator; subsequent `validated()` and `safe()` calls use that policy immediately.
