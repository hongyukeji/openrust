use std::path::{Path, PathBuf};

use jiff::civil::Date as CivilDate;

use super::{Builtin, Faker, arguments, flag, integer, text};
use crate::database::eloquent::factories::ImageOptions;
use crate::database::{DatabaseError, DatabaseResult, Value};

pub(super) fn formatter(name: &str) -> Option<Builtin> {
    Some(match name {
        "nino" => |faker, _| faker.nino().map(Value::from),
        "cpr" => |faker, _| faker.cpr().map(Value::from),
        "birthnumbermale" => |faker, _| faker.birth_number_male().map(Value::from),
        "birthnumberfemale" => |faker, _| faker.birth_number_female().map(Value::from),
        "ahv13" => |faker, _| faker.ahv13().map(Value::from),
        "avs13" => |faker, _| faker.avs13().map(Value::from),
        "nationalcode" => |faker, _| faker.national_code().map(Value::from),
        "taxid" => |faker, _| faker.tax_id().map(Value::from),
        "foreigneridnumber" => |faker, _| faker.foreigner_id_number().map(Value::from),
        "governorateid" => |faker, _| faker.governorate_id().map(Value::from),
        "driverlicence" => |faker, _| faker.driver_licence().map(Value::from),
        "passportnumber" => |faker, _| faker.passport_number().map(Value::from),
        "tcno" => |faker, _| faker.tc_no().map(Value::from),
        "doy" => |faker, _| faker.doy().map(Value::from),
        "object" => |faker, _| faker.object().map(Value::from),
        "companynumber" => |faker, _| faker.company_number().map(Value::from),
        "companyidnumber" => |faker, _| faker.company_id_number().map(Value::from),
        "companytaxidnumber" => |faker, _| faker.company_tax_id_number().map(Value::from),
        "companytraderegisternumber" => {
            |faker, _| faker.company_trade_register_number().map(Value::from)
        }
        "ico" => |faker, _| faker.ico().map(Value::from),
        "cvr" => |faker, _| faker.cvr().map(Value::from),
        "p" => |faker, _| faker.p().map(Value::from),
        "vsk" => |faker, _| faker.vsk().map(Value::from),
        "regon" => |faker, _| faker.regon().map(Value::from),
        "regonlocal" => |faker, _| faker.regon_local().map(Value::from),
        "ein" => |faker, _| faker.ein().map(Value::from),
        "dni" => |faker, _| faker.dni().map(Value::from),
        "licencecode" => |faker, _| faker.licence_code().map(Value::from),
        "file" => copy_file,
        "cnp" => |faker, args| {
            let [gender, birth, county, resident] = arguments(args)?;
            let resident = resident.map_or(Ok(true), |value| flag(Some(value)))?;
            faker
                .cnp(text(gender)?, text(birth)?, text(county)?, resident)
                .map(Value::from)
        },
        "mykadnumber" => |faker, args| {
            let [gender, hyphen] = arguments(args)?;
            let (formatted, separator) = match hyphen {
                Some(Value::Text(separator)) => (true, Some(separator.as_str())),
                value => (flag(value)?, None),
            };
            faker
                .my_kad_number(text(gender)?, formatted, separator)
                .map(Value::from)
        },
        "countrycodeprefix" => |faker, args| {
            let [formatting] = arguments(args)?;
            let formatting = formatting.map_or(Ok(true), |value| flag(Some(value)))?;
            faker.country_code_prefix(formatting).map(Value::from)
        },
        "calculateroutingnumberchecksum" => |_, args| {
            Ok(
                i64::from(Faker::calculate_routing_number_checksum(required_text(
                    args,
                )?)?)
                .into(),
            )
        },
        "calculatemodulus97" => |_, args| {
            let [input, use_9755] = arguments(args)?;
            let input =
                text(input)?.ok_or(DatabaseError::InvalidQuery("Faker formatter requires text"))?;
            let use_9755 = use_9755.map_or(Ok(true), |value| flag(Some(value)))?;
            Ok(format!("{:02}", Faker::calculate_modulus97(input, use_9755)?).into())
        },
        "imageurl" => |faker, args| {
            faker
                .image_url(&image_options(arguments(args)?)?)
                .map(Value::from)
        },
        "getformats" => |_, _| Ok(Value::Json(serde_json::json!(Faker::get_formats()))),
        "getformatconstants" => |_, _| {
            let formats = Faker::get_format_constants()
                .into_iter()
                .map(|(name, code)| (name.to_owned(), serde_json::Value::from(code)))
                .collect();
            Ok(Value::Json(serde_json::Value::Object(formats)))
        },
        "ssn" => |faker, args| {
            let [birth] = arguments(args)?;
            match date(birth)? {
                Some(birth) => faker.ssn_with_birthdate(Some(birth)).map(Value::from),
                None => faker.ssn().map(Value::from),
            }
        },
        "birthnumber" => |faker, args| {
            let [gender, min, max, slash] = arguments(args)?;
            faker
                .birth_number(
                    text(gender)?,
                    unsigned(min, 0)?,
                    unsigned(max, 100)?,
                    integer(slash)?.unwrap_or(50),
                )
                .map(Value::from)
        },
        "individualidentificationnumber" => |faker, args| {
            let [birth, gender] = arguments(args)?;
            faker
                .individual_identification_number(date(birth)?, text(gender)?)
                .map(Value::from)
        },
        "businessidentificationnumber" => |faker, args| {
            let [registration] = arguments(args)?;
            faker
                .business_identification_number(date(registration)?)
                .map(Value::from)
        },
        "nik" => |faker, args| {
            let [gender, birth] = arguments(args)?;
            faker.nik(text(gender)?, date(birth)?).map(Value::from)
        },
        "nir" => |faker, args| {
            let [gender, formatted] = arguments(args)?;
            faker.nir(text(gender)?, flag(formatted)?).map(Value::from)
        },
        "rrn" => |faker, args| {
            let [gender] = arguments(args)?;
            faker.rrn(text(gender)?).map(Value::from)
        },
        "pesel" => |faker, args| {
            let [birth, sex] = arguments(args)?;
            faker.pesel(date(birth)?, text(sex)?).map(Value::from)
        },
        "singaporeid" => |faker, args| {
            let [issue, foreigner] = arguments(args)?;
            let foreigner = match foreigner {
                None => false,
                Some(Value::Bool(value)) => *value,
                _ => {
                    return Err(DatabaseError::InvalidQuery(
                        "Faker formatter expects a boolean",
                    ));
                }
            };
            faker.singapore_id(date(issue)?, foreigner).map(Value::from)
        },
        "nric" => |faker, args| {
            let [birth] = arguments(args)?;
            faker.nric(date(birth)?).map(Value::from)
        },
        "fin" => |faker, args| {
            let [issue] = arguments(args)?;
            faker.fin(date(issue)?).map(Value::from)
        },
        "idnumber" => |faker, args| {
            let [birth, citizen, gender] = arguments(args)?;
            if faker.provider_locale() == "en_ZA" {
                faker
                    .id_number_with_birthdate(date(birth)?, optional_flag(citizen)?, text(gender)?)
                    .map(Value::from)
            } else if args.is_empty() {
                faker.id_number().map(Value::from)
            } else {
                Err(DatabaseError::InvalidQuery(
                    "Faker dated identity number is unsupported for this locale",
                ))
            }
        },
        "nationalidnumber" => |faker, args| {
            let [gender] = arguments(args)?;
            match text(gender)? {
                Some(gender) => faker.national_id_number_with_gender(Some(gender)),
                None => faker.national_id_number(),
            }
            .map(Value::from)
        },
        "personalidentitynumber" => personal_identity_number,
        "nationalid" => |faker, args| {
            let [separator] = arguments(args)?;
            faker.national_id(text(separator)?).map(Value::from)
        },
        "taxpayeridentificationnumber" => |faker, args| {
            let [separator] = arguments(args)?;
            match text(separator)? {
                Some(separator) => faker
                    .taxpayer_identification_number_with_separator(Some(separator))
                    .map(Value::from),
                None => faker.taxpayer_identification_number().map(Value::from),
            }
        },
        "siret" => |faker, args| {
            let [formatted] = arguments(args)?;
            faker.siret(optional_flag(formatted)?).map(Value::from)
        },
        "cpf" => |faker, args| {
            let [formatted] = arguments(args)?;
            faker.cpf(optional_flag(formatted)?).map(Value::from)
        },
        "rg" => |faker, args| {
            let [formatted] = arguments(args)?;
            faker.rg(optional_flag(formatted)?).map(Value::from)
        },
        "cnpj" => |faker, args| {
            let [formatted] = arguments(args)?;
            faker.cnpj(optional_flag(formatted)?).map(Value::from)
        },
        "ruc" => |faker, args| {
            let [natural] = arguments(args)?;
            faker.ruc(optional_flag(natural)?).map(Value::from)
        },
        "inn" => |faker, args| {
            let [area] = arguments(args)?;
            faker.inn(text(area)?).map(Value::from)
        },
        "inn10" => |faker, args| {
            let [area] = arguments(args)?;
            faker.inn10(text(area)?).map(Value::from)
        },
        "kpp" => |faker, args| {
            let [inn] = arguments(args)?;
            faker.kpp(text(inn)?).map(Value::from)
        },
        "checksum" => |_, args| Ok(i64::from(Faker::check_sum(required_text(args)?)?).into()),
        "tcnoisvalid" => |_, args| Faker::tc_no_is_valid(required_text(args)?).map(Value::from),
        "tcnochecksum" => |_, args| Faker::tc_no_checksum(required_text(args)?).map(Value::from),
        "inn10isvalid" => |_, args| Ok(Faker::inn10_is_valid(required_text(args)?).into()),
        "inn10checksum" => |_, args| Faker::inn10_checksum(required_text(args)?).map(Value::from),
        "dvcalcmod11" => {
            |_, args| Ok(i64::from(Faker::dv_calc_mod11(required_text(args)?)?).into())
        }
        _ => return None,
    })
}

fn copy_file(faker: &Faker, args: &[Value]) -> DatabaseResult<Value> {
    let [source, target, full] = arguments(args)?;
    let source = text(source)?.map(Path::new);
    let target = text(target)?.map(Path::new);
    let default_directory = std::env::temp_dir();
    let source = source.unwrap_or(&default_directory);
    let target = target.unwrap_or(&default_directory);
    let full = full.map_or(Ok(true), |value| flag(Some(value)))?;
    if full && target.to_str().is_none() {
        return Err(DatabaseError::InvalidQuery(
            "Faker file target is not UTF-8",
        ));
    }
    let path = faker
        .file(source, target, full)
        .map_err(DatabaseError::FakerIo)?;
    match path.into_os_string().into_string() {
        Ok(path) => Ok(path.into()),
        Err(path) => {
            let path = PathBuf::from(path);
            let copied = if full { path } else { target.join(path) };
            std::fs::remove_file(copied).map_err(DatabaseError::FakerIo)?;
            Err(DatabaseError::InvalidQuery("Faker file path is not UTF-8"))
        }
    }
}

fn required_text(args: &[Value]) -> DatabaseResult<&str> {
    let [value] = arguments(args)?;
    text(value)?.ok_or(DatabaseError::InvalidQuery("Faker formatter requires text"))
}

fn date(value: Option<&Value>) -> DatabaseResult<Option<CivilDate>> {
    text(value)?
        .map(|value| {
            CivilDate::strptime("%Y-%m-%d", value)
                .map_err(|_| DatabaseError::InvalidQuery("invalid Faker civil date"))
        })
        .transpose()
}

fn unsigned(value: Option<&Value>, default: u32) -> DatabaseResult<u32> {
    integer(value)?
        .map(|value| {
            u32::try_from(value)
                .map_err(|_| DatabaseError::InvalidQuery("Faker unsigned argument is out of range"))
        })
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn optional_flag(value: Option<&Value>) -> DatabaseResult<Option<bool>> {
    value
        .filter(|value| !matches!(value, Value::Null))
        .map(|value| flag(Some(value)))
        .transpose()
}

fn personal_identity_number(faker: &Faker, args: &[Value]) -> DatabaseResult<Value> {
    match faker.provider_locale() {
        "lt_LT" => {
            let [gender, birth, serial] = arguments(args)?;
            faker.personal_identity_number_with_serial(
                date(birth)?,
                text(gender)?,
                text(serial)?.filter(|value| !value.is_empty()),
            )
        }
        "lv_LV" => {
            let [birth] = arguments(args)?;
            faker.personal_identity_number_with_birthdate(date(birth)?, None)
        }
        "fi_FI" | "nb_NO" | "sv_SE" => {
            let [birth, gender] = arguments(args)?;
            faker.personal_identity_number_with_birthdate(date(birth)?, text(gender)?)
        }
        _ => {
            let [gender] = arguments(args)?;
            faker.personal_identity_number(text(gender)?)
        }
    }
    .map(Value::from)
}

fn image_options(args: [Option<&Value>; 7]) -> DatabaseResult<ImageOptions<'_>> {
    let [width, height, category, randomize, word, gray, format] = args;
    Ok(ImageOptions {
        width: unsigned(width, if width.is_none() { 640 } else { 0 })?,
        height: unsigned(height, if height.is_none() { 480 } else { 0 })?,
        category: text(category)?,
        randomize: optional_flag(randomize)?.unwrap_or(randomize.is_none()),
        word: text(word)?,
        gray: flag(gray)?,
        format: text(format)?.unwrap_or(if format.is_none() { "png" } else { "" }),
    })
}

pub(super) async fn image(faker: &Faker, args: &[Value]) -> DatabaseResult<Value> {
    let [
        directory,
        width,
        height,
        category,
        full_path,
        randomize,
        word,
        gray,
        format,
    ] = arguments(args)?;
    let directory = text(directory)?.map(Path::new);
    let full_path = optional_flag(full_path)?.unwrap_or(full_path.is_none());
    if full_path && directory.is_none() && std::env::temp_dir().to_str().is_none() {
        return Err(DatabaseError::InvalidQuery(
            "Faker image directory is not UTF-8",
        ));
    }
    let options = image_options([width, height, category, randomize, word, gray, format])?;
    let path = faker
        .image(directory, full_path, &options)
        .await
        .map_err(DatabaseError::FakerIo)?;
    path.map_or(Ok(Value::Null), image_path)
}

fn image_path(path: PathBuf) -> DatabaseResult<Value> {
    path.into_os_string()
        .into_string()
        .map(Value::from)
        .map_err(|_| DatabaseError::InvalidQuery("Faker image path is not UTF-8"))
}
