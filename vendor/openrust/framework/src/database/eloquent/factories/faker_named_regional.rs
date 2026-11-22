use super::{Builtin, Faker, arguments, collection_values, flag, integer, text};
use crate::{
    database::{DatabaseError, DatabaseResult, Value},
    support::calendar,
};

pub(super) fn formatter(name: &str) -> Option<Builtin> {
    Some(match name {
        "cityprefix" => |faker, _| Ok(faker.cityPrefix().into()),
        "citysuffix" => |faker, _| Ok(faker.citySuffix().into()),
        "streetname" => |faker, _| Ok(faker.streetName().into()),
        "streetsuffix" => |faker, _| Ok(faker.streetSuffix().into()),
        "streetaddress" => |faker, _| Ok(faker.streetAddress().into()),
        "buildingnumber" => |faker, _| Ok(faker.buildingNumber().into()),
        "secondaryaddress" => |faker, _| Ok(faker.secondaryAddress().into()),
        "country" => |faker, _| Ok(faker.country().into()),
        "state" => |faker, _| Ok(faker.state().into()),
        "stateabbr" => |faker, _| Ok(faker.stateAbbr().into()),
        "alphabet" => |faker, _| {
            faker
                .alphabet()
                .map(|character| character.to_string().into())
        },
        "bs" => |faker, _| faker.bs().map(Value::from),
        "catchphrase" => |faker, _| faker.catchPhrase().map(Value::from),
        "catchphraseattribute" => |faker, _| faker.catchPhraseAttribute().map(Value::from),
        "catchphrasenoun" => |faker, _| faker.catchPhraseNoun().map(Value::from),
        "catchphraseverb" => |faker, _| faker.catchPhraseVerb().map(Value::from),
        "companyfield" => |faker, _| faker.companyField().map(Value::from),
        "companyname" => |faker, _| faker.companyName().map(Value::from),
        "companynameelement" => |faker, _| faker.companyNameElement().map(Value::from),
        "companynamesuffix" => |faker, _| faker.companyNameSuffix().map(Value::from),
        "companytype" => |faker, _| faker.companyType().map(Value::from),
        "companyurl" => |faker, _| faker.companyUrl().map(Value::from),
        "contract" => |faker, _| faker.contract().map(Value::from),
        "firstnamefemalechinese" => |faker, _| faker.firstNameFemaleChinese().map(Value::from),
        "firstnamefemalechristian" => |faker, _| faker.firstNameFemaleChristian().map(Value::from),
        "firstnamefemaleindian" => |faker, _| faker.firstNameFemaleIndian().map(Value::from),
        "firstnamefemalemalay" => |faker, _| faker.firstNameFemaleMalay().map(Value::from),
        "firstnamemalechinese" => |faker, _| faker.firstNameMaleChinese().map(Value::from),
        "firstnamemalechristian" => |faker, _| faker.firstNameMaleChristian().map(Value::from),
        "firstnamemaleindian" => |faker, _| faker.firstNameMaleIndian().map(Value::from),
        "firstnamemalemalay" => |faker, _| faker.firstNameMaleMalay().map(Value::from),
        "haji" => |faker, _| faker.haji().map(Value::from),
        "hajjah" => |faker, _| faker.hajjah().map(Value::from),
        "industry" => |faker, _| faker.industry().map(Value::from),
        "initialindian" => |faker, _| faker.initialIndian().map(Value::from),
        "jobtitle" => |faker, _| Ok(faker.jobTitle().into()),
        "lastnamechinese" => |faker, _| faker.lastNameChinese().map(Value::from),
        "lastnameindian" => |faker, _| faker.lastNameIndian().map(Value::from),
        "lastnamemalay" => |faker, _| faker.lastNameMalay().map(Value::from),
        "muhammadname" => |faker, _| faker.muhammadName().map(Value::from),
        "nameprefix" => |faker, _| {
            faker
                .namePrefix()
                .map(|character| character.to_string().into())
        },
        "nurname" => |faker, _| faker.nurName().map(Value::from),
        "randomname" => random_name,
        "slogan" => |faker, _| faker.slogan().map(Value::from),
        "titlemalemalay" => |faker, _| faker.titleMaleMalay().map(Value::from),
        "firstnamemale" => |faker, _| Ok(faker.firstNameMale().into()),
        "firstnamefemale" => |faker, _| Ok(faker.firstNameFemale().into()),
        "titlemale" => |faker, _| Ok(faker.titleMale().into()),
        "titlefemale" => |faker, _| Ok(faker.titleFemale().into()),
        "companysuffix" => |faker, _| Ok(faker.companySuffix().into()),
        "creditcardtype" => |faker, _| Ok(faker.creditCardType().into()),
        "swiftbicnumber" => |faker, _| Ok(faker.swiftBicNumber().into()),
        "e164phonenumber" => |faker, _| Ok(faker.e164PhoneNumber().into()),
        "imei" => |faker, _| Ok(faker.imei().into()),
        "area" => |faker, _| faker.area().map(Value::from),
        "street" => |faker, _| faker.street().map(Value::from),
        "community" => |faker, _| faker.community().map(Value::from),
        "streetprefix" => |faker, _| faker.streetPrefix().map(Value::from),
        "streetnumber" => |faker, _| faker.streetNumber().map(Value::from),
        "blocknumber" => |faker, _| faker.blockNumber().map(Value::from),
        "floornumber" => |faker, _| faker.floorNumber().map(Value::from),
        "apartmentnumber" => |faker, _| faker.apartmentNumber().map(Value::from),
        "block" => |faker, _| faker.block().map(Value::from),
        "blocksegment" => |faker, _| faker.blockSegment().map(Value::from),
        "floor" => |faker, _| faker.floor().map(Value::from),
        "streetplainname" => |faker, _| faker.streetPlainName().map(Value::from),
        "townname" => |faker, _| faker.townName().map(Value::from),
        "locality" => |faker, _| faker.locality().map(Value::from),
        "localityname" => |faker, _| faker.localityName().map(Value::from),
        "areasuffix" => |faker, _| faker.areaSuffix().map(Value::from),
        "societyname" => |faker, _| faker.societyName().map(Value::from),
        "societysuffix" => |faker, _| faker.societySuffix().map(Value::from),
        "municipality" => |faker, _| faker.municipality().map(Value::from),
        "barangay" => |faker, _| faker.barangay().map(Value::from),
        "town" => |faker, _| faker.town().map(Value::from),
        "village" => |faker, _| faker.village().map(Value::from),
        "estate" => |faker, _| faker.estate().map(Value::from),
        "syllable" => |faker, _| faker.syllable().map(Value::from),
        "direction" => |faker, _| faker.direction().map(Value::from),
        "englishstreetname" => |faker, _| faker.englishStreetName().map(Value::from),
        "villagesuffix" => |faker, _| faker.villageSuffix().map(Value::from),
        "estatesuffix" => |faker, _| faker.estateSuffix().map(Value::from),
        "prefecture" => |faker, _| faker.prefecture().map(Value::from),
        "ward" => |faker, _| faker.ward().map(Value::from),
        "postcode1" => |faker, _| faker.postcode1().map(Value::from),
        "postcode2" => |faker, _| faker.postcode2().map(Value::from),
        "areanumber" => |faker, _| faker.areaNumber().map(Value::from),
        "metropolitancity" => |faker, _| faker.metropolitanCity().map(Value::from),
        "borough" => |faker, _| faker.borough().map(Value::from),
        "hamletname" => |faker, _| faker.hamletName().map(Value::from),
        "hamletprefix" => |faker, _| faker.hamletPrefix().map(Value::from),
        "wardname" => |faker, _| faker.wardName().map(Value::from),
        "wardprefix" => |faker, _| faker.wardPrefix().map(Value::from),
        "districtname" => |faker, _| faker.districtName().map(Value::from),
        "districtprefix" => |faker, _| faker.districtPrefix().map(Value::from),
        "buildingprefix" => |faker, _| faker.buildingPrefix().map(Value::from),
        "township" => |faker, _| faker.township().map(Value::from),
        "townshipprefixabbr" => |faker, _| faker.townshipPrefixAbbr().map(Value::from),
        "townshipprefix" => |faker, _| faker.townshipPrefix().map(Value::from),
        "townshipsuffix" => |faker, _| faker.townshipSuffix().map(Value::from),
        "townstate" => |faker, _| faker.townState().map(Value::from),
        "banglastreetname" => |faker, _| faker.banglaStreetName().map(Value::from),
        "wardnumber" => |faker, _| faker.wardNumber().map(Value::from),
        "governorate" => |faker, _| faker.governorate().map(Value::from),
        "subdivision" => |faker, _| faker.subdivision().map(Value::from),
        "buildingnameprefix" => |faker, _| faker.buildingNamePrefix().map(Value::from),
        "buildingnumberprefix" => |faker, _| faker.buildingNumberPrefix().map(Value::from),
        "postcodeprefix" => |faker, _| faker.postcodePrefix().map(Value::from),
        "building" => |faker, _| faker.building().map(Value::from),
        "regiongenitiveform" => |faker, _| faker.regionGenitiveForm().map(Value::from),
        "regionsuffix" => |faker, _| faker.regionSuffix().map(Value::from),
        "cityandregion" => |faker, _| faker.cityAndRegion().map(Value::from),
        "streetsuffixshort" => |faker, _| faker.streetSuffixShort().map(Value::from),
        "streetsuffixlong" => |faker, _| faker.streetSuffixLong().map(Value::from),
        "streetsuffixword" => |faker, _| faker.streetSuffixWord().map(Value::from),
        "buildinglevel" => |faker, _| faker.buildingLevel().map(Value::from),
        "buildingside" => |faker, _| faker.buildingSide().map(Value::from),
        "kommune" => |faker, _| faker.kommune().map(Value::from),
        "capital" => |faker, _| faker.capital().map(Value::from),
        "bigcity" => |faker, _| faker.bigCity().map(Value::from),
        "smallercity" => |faker, _| faker.smallerCity().map(Value::from),
        "cityname" => |faker, _| faker.cityName().map(Value::from),
        "county" => |faker, _| faker.county().map(Value::from),
        "province" => |faker, _| faker.province().map(Value::from),
        "provinceabbr" => |faker, _| faker.provinceAbbr().map(Value::from),
        "randompostcodeletter" => |faker, _| faker.randomPostcodeLetter().map(Value::from),
        "buildingletter" => |faker, _| faker.buildingLetter().map(Value::from),
        "cantonshort" => |faker, _| faker.cantonShort().map(Value::from),
        "cantonname" => |faker, _| faker.cantonName().map(Value::from),
        "district" => |faker, _| faker.district().map(Value::from),
        "regionabbr" => |faker, _| faker.regionAbbr().map(Value::from),
        "region" => |faker, _| faker.region().map(Value::from),
        "departmentname" => |faker, _| faker.departmentName().map(Value::from),
        "departmentnumber" => |faker, _| faker.departmentNumber().map(Value::from),
        "middlename" => |faker, _| faker.middleName().map(Value::from),
        "middlenamemale" => |faker, _| faker.middleNameMale().map(Value::from),
        "middlenamefemale" => |faker, _| faker.middleNameFemale().map(Value::from),
        "lastnamemale" => |faker, _| faker.lastNameMale().map(Value::from),
        "lastnamefemale" => |faker, _| faker.lastNameFemale().map(Value::from),
        "firstnamemalene" => |faker, _| faker.firstNameMaleNe().map(Value::from),
        "lastnamefemalemarried" => |faker, _| faker.lastNameFemaleMarried().map(Value::from),
        "firstnameascii" => |faker, _| faker.firstNameAscii().map(Value::from),
        "lastnameascii" => |faker, _| faker.lastNameAscii().map(Value::from),
        "prefix" => |faker, _| faker.prefix().map(Value::from),
        "firstkananamemale" => |faker, _| faker.firstKanaNameMale().map(Value::from),
        "firstkananamefemale" => |faker, _| faker.firstKanaNameFemale().map(Value::from),
        "lastkananame" => |faker, _| faker.lastKanaName().map(Value::from),
        "companyprefix" => |faker, _| faker.companyPrefix().map(Value::from),
        "companymodifier" => |faker, _| faker.companyModifier().map(Value::from),
        "companyensuffix" => |faker, _| faker.companyEnSuffix().map(Value::from),
        "bank" => |faker, _| faker.bank().map(Value::from),
        "microfinance" => |faker, _| faker.microFinance().map(Value::from),
        "swiftcode" => |faker, _| faker.swiftCode().map(Value::from),
        "bankroutingnumber" => |faker, _| faker.bankRoutingNumber().map(Value::from),
        "vatid" => |faker, _| faker.vatId().map(Value::from),
        "btw" => |faker, _| faker.btw().map(Value::from),
        "localbank" => |faker, _| faker.localBank().map(Value::from),
        "foreignbank" => |faker, _| faker.foreignBank().map(Value::from),
        "governmentbank" => |faker, _| faker.governmentBank().map(Value::from),
        "insurance" => |faker, _| faker.insurance().map(Value::from),
        "currencysymbol" => |faker, _| faker.currencySymbol().map(Value::from),
        "commercialbank" => |faker, _| faker.commercialBank().map(Value::from),
        "developmentbank" => |faker, _| faker.developmentBank().map(Value::from),
        "financecompany" => |faker, _| faker.financeCompany().map(Value::from),
        "digitalwallet" => |faker, _| faker.digitalWallet().map(Value::from),
        "phonenumbercleared" => |faker, _| faker.phoneNumberCleared().map(Value::from),
        "name" => |faker, args| gendered(faker, args, Faker::nameWithGender),
        "firstname" => |faker, args| gendered(faker, args, Faker::firstNameWithGender),
        "lastname" => |faker, args| gendered(faker, args, Faker::lastNameWithGender),
        "title" => |faker, args| gendered(faker, args, Faker::title),
        "middlenamewithgender" => |faker, args| {
            let [gender] = arguments(args)?;
            faker.middleNameWithGender(text(gender)?).map(Value::from)
        },
        "kananame" => |faker, args| {
            let [gender] = arguments(args)?;
            faker.kanaName(text(gender)?).map(Value::from)
        },
        "firstkananame" => |faker, args| {
            let [gender] = arguments(args)?;
            faker.firstKanaName(text(gender)?).map(Value::from)
        },
        "suffix" => |faker, _| Ok(faker.suffix()?.map_or(Value::Null, Value::from)),
        "postcode" => |faker, args| {
            let [state] = arguments(args)?;
            match text(state)? {
                Some(state) => faker.postcodeWithState(state).map(Value::from),
                None => Ok(faker.postcode().into()),
            }
        },
        "randomchinesenumber" => |faker, _| faker.randomChineseNumber().map(Value::from),
        "randomnumber2" => |faker, _| faker.randomNumber2().map(|number| i64::from(number).into()),
        "randomnumber3" => |faker, _| faker.randomNumber3().map(|number| i64::from(number).into()),
        "locallatitude" => |faker, _| taiwan_coordinate(faker, Faker::localLatitude),
        "locallongitude" => |faker, _| taiwan_coordinate(faker, Faker::localLongitude),
        "licenseplate" => license_plate,
        "jpjnumberplate" => |faker, _| faker.jpjNumberPlate().map(Value::from),
        "peninsularprefix" => |faker, _| faker.peninsularPrefix().map(Value::from),
        "sarawakprefix" => |faker, _| faker.sarawakPrefix().map(Value::from),
        "sabahprefix" => |faker, _| faker.sabahPrefix().map(Value::from),
        "specialprefix" => |faker, _| faker.specialPrefix().map(Value::from),
        "validalphabet" => |faker, _| faker.validAlphabet().map(Value::from),
        "numbersequence" => |faker, _| faker.numberSequence().map(Value::from),
        "localcoordinates" => |faker, _| {
            let coordinates = faker.localCoordinates()?;
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
                .creditCardNumber(text(kind)?, flag(formatted)?, text(separator)?)
                .map(Value::from)
        },
        "creditcardexpirationdate" => |faker, args| {
            let [valid] = arguments(args)?;
            let date = faker.creditCardExpirationDate(validity(valid)?)?;
            Ok(super::rfc3339_seconds(&date).into())
        },
        "creditcardexpirationdatestring" => |faker, args| {
            let [valid, format] = arguments(args)?;
            let date = faker.creditCardExpirationDate(validity(valid)?)?;
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
            let card = faker.creditCardDetails(validity(valid)?)?;
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
                .bankAccountNumber(
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
                faker.VAT().map(Value::from)
            } else if faker.provider_locale() == "en_GB" {
                faker.vatWithType(text(kind)?).map(Value::from)
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
        .licensePlateWithOptions(special, Some(&provinces), Some(&counties))
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
    faker.randomName(&pool, count).map(Value::from)
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
