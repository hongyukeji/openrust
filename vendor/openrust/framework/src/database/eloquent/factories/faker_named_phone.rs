use super::{Builtin, Faker, arguments, flag};
use crate::database::{DatabaseResult, Value};

pub(super) fn formatter(name: &str) -> Option<Builtin> {
    Some(match name {
        "mobilenumber" => |faker, args| {
            phone_with_prefix(
                faker,
                args,
                Faker::mobileNumber,
                Faker::mobileNumberWithOptions,
            )
        },
        "fixedlinenumber" => |faker, args| {
            phone_with_prefix(
                faker,
                args,
                Faker::fixedLineNumber,
                Faker::fixedLineNumberWithOptions,
            )
        },
        "voipnumber" => |faker, args| {
            phone_with_prefix(faker, args, Faker::voipNumber, Faker::voipNumberWithOptions)
        },
        "cellphonenumber" => |faker, args| {
            let formatted = brazilian_formatting(faker, args)?;
            faker.cellphoneNumber(formatted).map(Value::from)
        },
        "landlinenumber" => |faker, args| {
            let formatted = brazilian_formatting(faker, args)?;
            faker.landlineNumber(formatted).map(Value::from)
        },
        "cellphone" => |faker, args| {
            let formatted = brazilian_formatting(faker, args)?;
            faker.cellphone(formatted).map(Value::from)
        },
        "landline" => |faker, args| {
            let formatted = brazilian_formatting(faker, args)?;
            faker.landline(formatted).map(Value::from)
        },
        "phone" => |faker, args| {
            let formatted = brazilian_formatting(faker, args)?;
            faker.phone(formatted).map(Value::from)
        },
        "mobilephonenumber" => |faker, _| faker.mobilePhoneNumber().map(Value::from),
        "phonenumberwithextension" => |faker, _| faker.phoneNumberWithExtension().map(Value::from),
        "tollfreephonenumber" => |faker, _| faker.tollFreePhoneNumber().map(Value::from),
        "areacode" => |faker, _| faker.areaCode().map(Value::from),
        "exchangecode" => |faker, _| faker.exchangeCode().map(Value::from),
        "tollfreeareacode" => |faker, _| faker.tollFreeAreaCode().map(Value::from),
        "numberformat" => |faker, _| faker.numberFormat().map(Value::from),
        "separator" => |faker, _| faker.separator().map(Value::from),
        "mobilecode" => |faker, _| faker.mobileCode().map(Value::from),
        "internationalcodeprefix" => |faker, _| faker.internationalCodePrefix().map(Value::from),
        "sharedcostcode" => |faker, _| faker.sharedCostCode().map(Value::from),
        "premiumratecode" => |faker, _| faker.premiumRateCode().map(Value::from),
        "cellphonecode" => |faker, _| faker.cellphoneCode().map(Value::from),
        "specialcode" => |faker, _| faker.specialCode().map(Value::from),
        "beginningnumber" => |faker, _| faker.beginningNumber().map(Value::from),
        "code" => |faker, _| faker.code().map(Value::from),
        "landlineareacode" => |faker, _| faker.landLineAreaCode().map(Value::from),
        "e164landlineareacode" => |faker, _| faker.e164landLineAreaCode().map(Value::from),
        "mobilenetworkareacode" => |faker, _| faker.mobileNetworkAreaCode().map(Value::from),
        "e164mobilenetworkareacode" => {
            |faker, _| faker.e164MobileNetworkAreaCode().map(Value::from)
        }
        "zerooneoneprefix" => |faker, _| faker.zeroOneOnePrefix().map(Value::from),
        "zeroonefourprefix" => |faker, _| faker.zeroOneFourPrefix().map(Value::from),
        "zeroonefiveprefix" => |faker, _| faker.zeroOneFivePrefix().map(Value::from),
        "onetoeight" => |faker, _| faker.oneToEight().map(Value::from),
        "zerotoeight" => |faker, _| faker.zeroToEight().map(Value::from),
        "phonenumber06" => |faker, _| faker.phoneNumber06().map(Value::from),
        "phonenumber06withseparator" => {
            |faker, _| faker.phoneNumber06WithSeparator().map(Value::from)
        }
        "phonenumber07" => |faker, _| faker.phoneNumber07().map(Value::from),
        "phonenumber07withseparator" => {
            |faker, _| faker.phoneNumber07WithSeparator().map(Value::from)
        }
        "phonenumber08" => |faker, _| faker.phoneNumber08().map(Value::from),
        "phonenumber08withseparator" => {
            |faker, _| faker.phoneNumber08WithSeparator().map(Value::from)
        }
        "faxnumber" => |faker, _| faker.faxNumber().map(Value::from),
        "localareaphonenumber" => |faker, _| faker.localAreaPhoneNumber().map(Value::from),
        "tollfreenumber" => |faker, _| faker.tollFreeNumber().map(Value::from),
        "personalnumber" => |faker, _| faker.personalNumber().map(Value::from),
        "sharedcostnumber" => |faker, _| faker.sharedCostNumber().map(Value::from),
        "premiumratenumber" => |faker, _| faker.premiumRateNumber().map(Value::from),
        "premiumratephonenumber" => |faker, _| faker.premiumRatePhoneNumber().map(Value::from),
        "premiumphonenumber" => |faker, _| faker.premiumPhoneNumber().map(Value::from),
        "tollfreeinternationalnumber" => {
            |faker, _| faker.tollFreeInternationalNumber().map(Value::from)
        }
        "tollfreelinenumber" => |faker, _| faker.tollFreeLineNumber().map(Value::from),
        "servicenumber" => |faker, _| faker.serviceNumber().map(Value::from),
        _ => return None,
    })
}

fn phone_with_prefix(
    faker: &Faker,
    args: &[Value],
    default: fn(&Faker) -> DatabaseResult<String>,
    with_options: fn(&Faker, bool, bool) -> DatabaseResult<String>,
) -> DatabaseResult<Value> {
    if faker.provider_locale() != "ms_MY" {
        arguments::<0>(args)?;
        return default(faker).map(Value::from);
    }
    let [prefix, formatting] = arguments(args)?;
    let prefix = prefix.map_or(Ok(true), |value| flag(Some(value)))?;
    let formatting = formatting.map_or(Ok(true), |value| flag(Some(value)))?;
    with_options(faker, prefix, formatting).map(Value::from)
}

fn brazilian_formatting(faker: &Faker, args: &[Value]) -> DatabaseResult<Option<bool>> {
    if faker.provider_locale() != "pt_BR" {
        arguments::<0>(args)?;
        return Ok(None);
    }
    let [formatted] = arguments(args)?;
    formatted.map(|value| flag(Some(value))).transpose()
}
