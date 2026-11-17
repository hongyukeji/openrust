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

In string rules, quote parameter values that contain commas, for example `exists:directory.references,reference,namespace,"team,west"`. A text exclusion ID such as `001` keeps its leading zeros. Array existence checks use Laravel's scalar string representation to count distinct inputs.

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

Use `dimensions` to constrain width, height, or aspect ratio. Ratios can be decimals or fractions such as `16/9`. SVG accepted by the image rule bypasses raster dimension checks. Use `encoding:UTF-8` or `File::default().encoding("UTF-8")` to check a file's bytes. Use supported Laravel encoding names such as UTF-8, ASCII, UTF-16, UTF-32, SJIS, CP932, or EUC-CN. Choose CP932 or SJIS-win for Windows Shift-JIS extensions.

For single-byte uploads, use `encoding:CP850` or `File::default().encoding("ArmSCII-8")`. CP850 accepts every byte. ArmSCII-8 and the Windows or ISO encodings with undefined positions reject those bytes according to PHP mbstring. Encoding validation preserves the uploaded bytes.

For East Asian double-byte uploads, choose the encoding used by the file. `encoding:BIG-5` and `encoding:EUC-KR` check the standard byte ranges. Use `File::default().encoding("CP950")` or `File::default().encoding("UHC")` for the corresponding Windows extensions. Each encoding rejects incomplete pairs and unassigned byte positions according to PHP mbstring.

Use `encoding:EUC-CN` for its assigned two-byte positions and `encoding:CP936` for the Windows byte repertoire. `encoding:GB18030` and `File::default().encoding("GB18030-2022")` follow PHP mbstring’s byte validation. Both accept the same two-byte and four-byte sequences.

For TIFF and ICO uploads, combine `file` with a size rule such as `dimensions:width=100,height=100`. TIFF dimensions come from the first image directory. ICO dimensions come from the last entry with the highest bit depth. These checks read metadata without decoding pixels. Laravel’s `image` rule excludes TIFF and ICO.

PSD and JPEG 2000 uploads can use the same rules. For a JP2 file, dimensions come from its first root-level codestream. For a raw JPEG 2000 codestream, dimensions come from its SIZ marker. Both formats are outside Laravel’s `image` rule.

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
