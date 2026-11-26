use super::{Builtin, Faker, arguments, collection_values, flag, integer, text};
use crate::{
    database::{DatabaseError, DatabaseResult, Value},
    support::calendar,
};

pub(super) fn formatter(name: &str) -> Option<Builtin> {
    Some(match name {
        "cityprefix" => |faker, _| Ok(faker.city_prefix().into()),
        "citysuffix" => |faker, _| Ok(faker.city_suffix().into()),
        "streetname" => |faker, _| Ok(faker.street_name().into()),
        "streetsuffix" => |faker, _| Ok(faker.street_suffix().into()),
        "streetaddress" => |faker, _| Ok(faker.street_address().into()),
        "buildingnumber" => |faker, _| Ok(faker.building_number().into()),
        "secondaryaddress" => |faker, _| Ok(faker.secondary_address().into()),
        "country" => |faker, _| Ok(faker.country().into()),
        "state" => |faker, _| Ok(faker.state().into()),
        "stateabbr" => |faker, _| Ok(faker.state_abbr().into()),
        "alphabet" => |faker, _| {
            faker
                .alphabet()
                .map(|character| character.to_string().into())
        },
        "bs" => |faker, _| faker.bs().map(Value::from),
        "catchphrase" => |faker, _| faker.catch_phrase().map(Value::from),
        "catchphraseattribute" => |faker, _| faker.catch_phrase_attribute().map(Value::from),
        "catchphrasenoun" => |faker, _| faker.catch_phrase_noun().map(Value::from),
        "catchphraseverb" => |faker, _| faker.catch_phrase_verb().map(Value::from),
        "companyfield" => |faker, _| faker.company_field().map(Value::from),
        "companyname" => |faker, _| faker.company_name().map(Value::from),
        "companynameelement" => |faker, _| faker.company_name_element().map(Value::from),
        "companynamesuffix" => |faker, _| faker.company_name_suffix().map(Value::from),
        "companytype" => |faker, _| faker.company_type().map(Value::from),
        "companyurl" => |faker, _| faker.company_url().map(Value::from),
        "contract" => |faker, _| faker.contract().map(Value::from),
        "firstnamefemalechinese" => |faker, _| faker.first_name_female_chinese().map(Value::from),
        "firstnamefemalechristian" => {
            |faker, _| faker.first_name_female_christian().map(Value::from)
        }
        "firstnamefemaleindian" => |faker, _| faker.first_name_female_indian().map(Value::from),
        "firstnamefemalemalay" => |faker, _| faker.first_name_female_malay().map(Value::from),
        "firstnamemalechinese" => |faker, _| faker.first_name_male_chinese().map(Value::from),
        "firstnamemalechristian" => |faker, _| faker.first_name_male_christian().map(Value::from),
        "firstnamemaleindian" => |faker, _| faker.first_name_male_indian().map(Value::from),
        "firstnamemalemalay" => |faker, _| faker.first_name_male_malay().map(Value::from),
        "haji" => |faker, _| faker.haji().map(Value::from),
        "hajjah" => |faker, _| faker.hajjah().map(Value::from),
        "industry" => |faker, _| faker.industry().map(Value::from),
        "initialindian" => |faker, _| faker.initial_indian().map(Value::from),
        "jobtitle" => |faker, _| Ok(faker.job_title().into()),
        "lastnamechinese" => |faker, _| faker.last_name_chinese().map(Value::from),
        "lastnameindian" => |faker, _| faker.last_name_indian().map(Value::from),
        "lastnamemalay" => |faker, _| faker.last_name_malay().map(Value::from),
        "muhammadname" => |faker, _| faker.muhammad_name().map(Value::from),
        "nameprefix" => |faker, _| {
            faker
                .name_prefix()
                .map(|character| character.to_string().into())
        },
        "nurname" => |faker, _| faker.nur_name().map(Value::from),
        "randomname" => random_name,
        "slogan" => |faker, _| faker.slogan().map(Value::from),
        "titlemalemalay" => |faker, _| faker.title_male_malay().map(Value::from),
        "firstnamemale" => |faker, _| Ok(faker.first_name_male().into()),
        "firstnamefemale" => |faker, _| Ok(faker.first_name_female().into()),
        "titlemale" => |faker, _| Ok(faker.title_male().into()),
        "titlefemale" => |faker, _| Ok(faker.title_female().into()),
        "companysuffix" => |faker, _| Ok(faker.company_suffix().into()),
        "creditcardtype" => |faker, _| Ok(faker.credit_card_type().into()),
        "swiftbicnumber" => |faker, _| Ok(faker.swift_bic_number().into()),
        "e164phonenumber" => |faker, _| Ok(faker.e164_phone_number().into()),
        "imei" => |faker, _| Ok(faker.imei().into()),
        "area" => |faker, _| faker.area().map(Value::from),
        "street" => |faker, _| faker.street().map(Value::from),
        "community" => |faker, _| faker.community().map(Value::from),
        "streetprefix" => |faker, _| faker.street_prefix().map(Value::from),
        "streetnumber" => |faker, _| faker.street_number().map(Value::from),
        "blocknumber" => |faker, _| faker.block_number().map(Value::from),
        "floornumber" => |faker, _| faker.floor_number().map(Value::from),
        "apartmentnumber" => |faker, _| faker.apartment_number().map(Value::from),
        "block" => |faker, _| faker.block().map(Value::from),
        "blocksegment" => |faker, _| faker.block_segment().map(Value::from),
        "floor" => |faker, _| faker.floor().map(Value::from),
        "streetplainname" => |faker, _| faker.street_plain_name().map(Value::from),
        "townname" => |faker, _| faker.town_name().map(Value::from),
        "locality" => |faker, _| faker.locality().map(Value::from),
        "localityname" => |faker, _| faker.locality_name().map(Value::from),
        "areasuffix" => |faker, _| faker.area_suffix().map(Value::from),
        "societyname" => |faker, _| faker.society_name().map(Value::from),
        "societysuffix" => |faker, _| faker.society_suffix().map(Value::from),
        "municipality" => |faker, _| faker.municipality().map(Value::from),
        "barangay" => |faker, _| faker.barangay().map(Value::from),
        "town" => |faker, _| faker.town().map(Value::from),
        "village" => |faker, _| faker.village().map(Value::from),
        "estate" => |faker, _| faker.estate().map(Value::from),
        "syllable" => |faker, _| faker.syllable().map(Value::from),
        "direction" => |faker, _| faker.direction().map(Value::from),
        "englishstreetname" => |faker, _| faker.english_street_name().map(Value::from),
        "villagesuffix" => |faker, _| faker.village_suffix().map(Value::from),
        "estatesuffix" => |faker, _| faker.estate_suffix().map(Value::from),
        "prefecture" => |faker, _| faker.prefecture().map(Value::from),
        "ward" => |faker, _| faker.ward().map(Value::from),
        "postcode1" => |faker, _| faker.postcode1().map(Value::from),
        "postcode2" => |faker, _| faker.postcode2().map(Value::from),
        "areanumber" => |faker, _| faker.area_number().map(Value::from),
        "metropolitancity" => |faker, _| faker.metropolitan_city().map(Value::from),
        "borough" => |faker, _| faker.borough().map(Value::from),
        "hamletname" => |faker, _| faker.hamlet_name().map(Value::from),
        "hamletprefix" => |faker, _| faker.hamlet_prefix().map(Value::from),
        "wardname" => |faker, _| faker.ward_name().map(Value::from),
        "wardprefix" => |faker, _| faker.ward_prefix().map(Value::from),
        "districtname" => |faker, _| faker.district_name().map(Value::from),
        "districtprefix" => |faker, _| faker.district_prefix().map(Value::from),
        "buildingprefix" => |faker, _| faker.building_prefix().map(Value::from),
        "township" => |faker, _| faker.township().map(Value::from),
        "townshipprefixabbr" => |faker, _| faker.township_prefix_abbr().map(Value::from),
        "townshipprefix" => |faker, _| faker.township_prefix().map(Value::from),
        "townshipsuffix" => |faker, _| faker.township_suffix().map(Value::from),
        "townstate" => |faker, _| faker.town_state().map(Value::from),
        "banglastreetname" => |faker, _| faker.bangla_street_name().map(Value::from),
        "wardnumber" => |faker, _| faker.ward_number().map(Value::from),
        "governorate" => |faker, _| faker.governorate().map(Value::from),
        "subdivision" => |faker, _| faker.subdivision().map(Value::from),
        "buildingnameprefix" => |faker, _| faker.building_name_prefix().map(Value::from),
        "buildingnumberprefix" => |faker, _| faker.building_number_prefix().map(Value::from),
        "postcodeprefix" => |faker, _| faker.postcode_prefix().map(Value::from),
        "building" => |faker, _| faker.building().map(Value::from),
        "regiongenitiveform" => |faker, _| faker.region_genitive_form().map(Value::from),
        "regionsuffix" => |faker, _| faker.region_suffix().map(Value::from),
        "cityandregion" => |faker, _| faker.city_and_region().map(Value::from),
        "streetsuffixshort" => |faker, _| faker.street_suffix_short().map(Value::from),
        "streetsuffixlong" => |faker, _| faker.street_suffix_long().map(Value::from),
        "streetsuffixword" => |faker, _| faker.street_suffix_word().map(Value::from),
        "buildinglevel" => |faker, _| faker.building_level().map(Value::from),
        "buildingside" => |faker, _| faker.building_side().map(Value::from),
        "kommune" => |faker, _| faker.kommune().map(Value::from),
        "capital" => |faker, _| faker.capital().map(Value::from),
        "bigcity" => |faker, _| faker.big_city().map(Value::from),
        "smallercity" => |faker, _| faker.smaller_city().map(Value::from),
        "cityname" => |faker, _| faker.city_name().map(Value::from),
        "county" => |faker, _| faker.county().map(Value::from),
        "province" => |faker, _| faker.province().map(Value::from),
        "provinceabbr" => |faker, _| faker.province_abbr().map(Value::from),
        "randompostcodeletter" => |faker, _| faker.random_postcode_letter().map(Value::from),
        "buildingletter" => |faker, _| faker.building_letter().map(Value::from),
        "cantonshort" => |faker, _| faker.canton_short().map(Value::from),
        "cantonname" => |faker, _| faker.canton_name().map(Value::from),
        "district" => |faker, _| faker.district().map(Value::from),
        "regionabbr" => |faker, _| faker.region_abbr().map(Value::from),
        "region" => |faker, _| faker.region().map(Value::from),
        "departmentname" => |faker, _| faker.department_name().map(Value::from),
        "departmentnumber" => |faker, _| faker.department_number().map(Value::from),
        "middlename" => |faker, _| faker.middle_name().map(Value::from),
        "middlenamemale" => |faker, _| faker.middle_name_male().map(Value::from),
        "middlenamefemale" => |faker, _| faker.middle_name_female().map(Value::from),
        "lastnamemale" => |faker, _| faker.last_name_male().map(Value::from),
        "lastnamefemale" => |faker, _| faker.last_name_female().map(Value::from),
        "firstnamemalene" => |faker, _| faker.first_name_male_ne().map(Value::from),
        "lastnamefemalemarried" => |faker, _| faker.last_name_female_married().map(Value::from),
        "firstnameascii" => |faker, _| faker.first_name_ascii().map(Value::from),
        "lastnameascii" => |faker, _| faker.last_name_ascii().map(Value::from),
        "prefix" => |faker, _| faker.prefix().map(Value::from),
        "firstkananamemale" => |faker, _| faker.first_kana_name_male().map(Value::from),
        "firstkananamefemale" => |faker, _| faker.first_kana_name_female().map(Value::from),
        "lastkananame" => |faker, _| faker.last_kana_name().map(Value::from),
        "companyprefix" => |faker, _| faker.company_prefix().map(Value::from),
        "companymodifier" => |faker, _| faker.company_modifier().map(Value::from),
        "companyensuffix" => |faker, _| faker.company_en_suffix().map(Value::from),
        "bank" => |faker, _| faker.bank().map(Value::from),
        "microfinance" => |faker, _| faker.micro_finance().map(Value::from),
        "swiftcode" => |faker, _| faker.swift_code().map(Value::from),
        "bankroutingnumber" => |faker, _| faker.bank_routing_number().map(Value::from),
        "vatid" => |faker, _| faker.vat_id().map(Value::from),
        "btw" => |faker, _| faker.btw().map(Value::from),
        "localbank" => |faker, _| faker.local_bank().map(Value::from),
        "foreignbank" => |faker, _| faker.foreign_bank().map(Value::from),
        "governmentbank" => |faker, _| faker.government_bank().map(Value::from),
        "insurance" => |faker, _| faker.insurance().map(Value::from),
        "currencysymbol" => |faker, _| faker.currency_symbol().map(Value::from),
        "commercialbank" => |faker, _| faker.commercial_bank().map(Value::from),
        "developmentbank" => |faker, _| faker.development_bank().map(Value::from),
        "financecompany" => |faker, _| faker.finance_company().map(Value::from),
        "digitalwallet" => |faker, _| faker.digital_wallet().map(Value::from),
        "phonenumbercleared" => |faker, _| faker.phone_number_cleared().map(Value::from),
        "name" => |faker, args| gendered(faker, args, Faker::name_with_gender),
        "firstname" => |faker, args| gendered(faker, args, Faker::first_name_with_gender),
        "lastname" => |faker, args| gendered(faker, args, Faker::last_name_with_gender),
        "title" => |faker, args| gendered(faker, args, Faker::title),
        "middlenamewithgender" => |faker, args| {
            let [gender] = arguments(args)?;
            faker
                .middle_name_with_gender(text(gender)?)
                .map(Value::from)
        },
        "kananame" => |faker, args| {
            let [gender] = arguments(args)?;
            faker.kana_name(text(gender)?).map(Value::from)
        },
        "firstkananame" => |faker, args| {
            let [gender] = arguments(args)?;
            faker.first_kana_name(text(gender)?).map(Value::from)
        },
        "suffix" => |faker, _| Ok(faker.suffix()?.map_or(Value::Null, Value::from)),
        "postcode" => |faker, args| {
            let [state] = arguments(args)?;
            match text(state)? {
                Some(state) => faker.postcode_with_state(state).map(Value::from),
                None => Ok(faker.postcode().into()),
            }
        },
        "randomchinesenumber" => |faker, _| faker.random_chinese_number().map(Value::from),
        "randomnumber2" => |faker, _| {
            faker
                .random_number2()
                .map(|number| i64::from(number).into())
        },
        "randomnumber3" => |faker, _| {
            faker
                .random_number3()
                .map(|number| i64::from(number).into())
        },
        "locallatitude" => |faker, _| taiwan_coordinate(faker, Faker::local_latitude),
        "locallongitude" => |faker, _| taiwan_coordinate(faker, Faker::local_longitude),
        "licenseplate" => license_plate,
        "jpjnumberplate" => |faker, _| faker.jpj_number_plate().map(Value::from),
        "peninsularprefix" => |faker, _| faker.peninsular_prefix().map(Value::from),
        "sarawakprefix" => |faker, _| faker.sarawak_prefix().map(Value::from),
        "sabahprefix" => |faker, _| faker.sabah_prefix().map(Value::from),
        "specialprefix" => |faker, _| faker.special_prefix().map(Value::from),
        "validalphabet" => |faker, _| faker.valid_alphabet().map(Value::from),
        "numbersequence" => |faker, _| faker.number_sequence().map(Value::from),
        "localcoordinates" => |faker, _| {
            let coordinates = faker.local_coordinates()?;
            Ok(Value::Json(serde_json::json!(coordinates)))
        },
        "canton" => |faker, _| {
            let (code, name) = faker.canton()?;
            Ok(Value::Json(serde_json::json!([code, name])))
        },
        "department" => |faker, _| {
            let (number, name) = faker.department()?;
            Ok(Value::Json(serde_json::json!([number, name])))
        },
        "creditcardnumber" => |faker, args| {
            let [kind, formatted, separator] = arguments(args)?;
            faker
                .credit_card_number(text(kind)?, flag(formatted)?, text(separator)?)
                .map(Value::from)
        },
        "creditcardexpirationdate" => |faker, args| {
            let [valid] = arguments(args)?;
            let date = faker.credit_card_expiration_date(validity(valid)?)?;
            Ok(super::rfc3339_seconds(&date).into())
        },
        "creditcardexpirationdatestring" => |faker, args| {
            let [valid, format] = arguments(args)?;
            let date = faker.credit_card_expiration_date(validity(valid)?)?;
            calendar::format_php_date(
                date.timestamp(),
                date.time_zone().clone(),
                text(format)?.unwrap_or("m/y"),
            )
            .map(Value::from)
            .map_err(|_| DatabaseError::InvalidQuery("Faker PHP date format could not be rendered"))
        },
        "creditcarddetails" => |faker, args| {
            let [valid] = arguments(args)?;
            let card = faker.credit_card_details(validity(valid)?)?;
            Ok(Value::Json(serde_json::json!(card)))
        },
        "iban" => |faker, args| {
            let [country, prefix, length] = arguments(args)?;
            faker
                .iban(
                    text(country)?,
                    text(prefix)?.unwrap_or(""),
                    account_length(length)?,
                )
                .map(Value::from)
        },
        "bankaccountnumber" => |faker, args| {
            let [prefix, country, length] = arguments(args)?;
            faker
                .bank_account_number(
                    text(prefix)?.unwrap_or(""),
                    text(country)?,
                    account_length(length)?,
                )
                .map(Value::from)
        },
        "siren" => |faker, args| {
            let [formatted] = arguments(args)?;
            faker.siren(validity(formatted)?).map(Value::from)
        },
        "vat" => |faker, args| {
            let [kind] = arguments(args)?;
            if faker.provider_locale() == "zh_TW" {
                faker.taiwan_vat().map(Value::from)
            } else if faker.provider_locale() == "en_GB" {
                faker.vat_with_type(text(kind)?).map(Value::from)
            } else {
                faker
                    .vat(kind.map(|value| flag(Some(value))).transpose()?)
                    .map(Value::from)
            }
        },
        _ => return None,
    })
}

fn gendered<T: Into<Value>>(
    faker: &Faker,
    args: &[Value],
    generate: impl FnOnce(&Faker, Option<&str>) -> T,
) -> DatabaseResult<Value> {
    let [gender] = arguments(args)?;
    Ok(generate(faker, text(gender)?).into())
}

fn validity(value: Option<&Value>) -> DatabaseResult<bool> {
    value.map_or(Ok(true), |value| flag(Some(value)))
}

fn account_length(value: Option<&Value>) -> DatabaseResult<Option<usize>> {
    integer(value)?
        .map(|length| {
            usize::try_from(length)
                .map_err(|_| DatabaseError::InvalidQuery("Faker account length is out of range"))
        })
        .transpose()
}

fn taiwan_coordinate(
    faker: &Faker,
    generate: fn(&Faker) -> DatabaseResult<f64>,
) -> DatabaseResult<Value> {
    if faker.provider_locale() != "zh_TW" {
        return Err(DatabaseError::InvalidQuery(
            "unsupported Faker Taiwanese coordinates locale",
        ));
    }
    generate(faker).map(Value::Real)
}

fn license_plate(faker: &Faker, args: &[Value]) -> DatabaseResult<Value> {
    let [special, provinces, counties] = arguments(args)?;
    let special = match special {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => {
            return Err(DatabaseError::InvalidQuery(
                "Faker formatter expects a boolean",
            ));
        }
    };
    let provinces = plate_area_names(provinces)?;
    let counties = plate_area_names(counties)?;
    faker
        .license_plate_with_options(special, Some(&provinces), Some(&counties))
        .map(Value::from)
}

fn plate_area_names(input: Option<&Value>) -> DatabaseResult<Vec<&str>> {
    if matches!(input, None | Some(Value::Null)) {
        return Ok(Vec::new());
    }
    collection_values(input, &[])?
        .into_iter()
        .map(|name| {
            name.as_str().ok_or(DatabaseError::InvalidQuery(
                "Faker plate area names must be text",
            ))
        })
        .collect()
}

fn random_name(faker: &Faker, args: &[Value]) -> DatabaseResult<Value> {
    let [pool, count] = arguments(args)?;
    let count = integer(count)?.ok_or(DatabaseError::InvalidQuery(
        "Faker randomName requires an integer count",
    ))?;
    let count = usize::try_from(count.max(0))
        .map_err(|_| DatabaseError::InvalidQuery("Faker name length is out of range"))?;
    let pool = required_text_collection(pool)?;
    faker.random_name(&pool, count).map(Value::from)
}

fn required_text_collection(value: Option<&Value>) -> DatabaseResult<Vec<&str>> {
    let value = value.ok_or(DatabaseError::InvalidQuery(
        "Faker randomName requires a name pool",
    ))?;
    collection_values(Some(value), &[])?
        .into_iter()
        .map(|name| {
            name.as_str().ok_or(DatabaseError::InvalidQuery(
                "Faker name pool entries must be text",
            ))
        })
        .collect()
}
