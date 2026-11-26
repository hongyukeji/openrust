use super::{Builtin, Faker, arguments, flag};
use crate::database::{DatabaseResult, Value};

pub(super) fn formatter(name: &str) -> Option<Builtin> {
    Some(match name {
        "mobilenumber" => |faker, args| {
            phone_with_prefix(
                faker,
                args,
                Faker::mobile_number,
                Faker::mobile_number_with_options,
            )
        },
        "fixedlinenumber" => |faker, args| {
            phone_with_prefix(
                faker,
                args,
                Faker::fixed_line_number,
                Faker::fixed_line_number_with_options,
            )
        },
        "voipnumber" => |faker, args| {
            phone_with_prefix(
                faker,
                args,
                Faker::voip_number,
                Faker::voip_number_with_options,
            )
        },
        "cellphonenumber" => |faker, args| {
            let formatted = brazilian_formatting(faker, args)?;
            faker.cellphone_number(formatted).map(Value::from)
        },
        "landlinenumber" => |faker, args| {
            let formatted = brazilian_formatting(faker, args)?;
            faker.landline_number(formatted).map(Value::from)
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
        "mobilephonenumber" => |faker, _| faker.mobile_phone_number().map(Value::from),
        "phonenumberwithextension" => {
            |faker, _| faker.phone_number_with_extension().map(Value::from)
        }
        "tollfreephonenumber" => |faker, _| faker.toll_free_phone_number().map(Value::from),
        "areacode" => |faker, _| faker.area_code().map(Value::from),
        "exchangecode" => |faker, _| faker.exchange_code().map(Value::from),
        "tollfreeareacode" => |faker, _| faker.toll_free_area_code().map(Value::from),
        "numberformat" => |faker, _| faker.number_format().map(Value::from),
        "separator" => |faker, _| faker.separator().map(Value::from),
        "mobilecode" => |faker, _| faker.mobile_code().map(Value::from),
        "internationalcodeprefix" => |faker, _| faker.international_code_prefix().map(Value::from),
        "sharedcostcode" => |faker, _| faker.shared_cost_code().map(Value::from),
        "premiumratecode" => |faker, _| faker.premium_rate_code().map(Value::from),
        "cellphonecode" => |faker, _| faker.cellphone_code().map(Value::from),
        "specialcode" => |faker, _| faker.special_code().map(Value::from),
        "beginningnumber" => |faker, _| faker.beginning_number().map(Value::from),
        "code" => |faker, _| faker.code().map(Value::from),
        "landlineareacode" => |faker, _| faker.land_line_area_code().map(Value::from),
        "e164landlineareacode" => |faker, _| faker.e164land_line_area_code().map(Value::from),
        "mobilenetworkareacode" => |faker, _| faker.mobile_network_area_code().map(Value::from),
        "e164mobilenetworkareacode" => {
            |faker, _| faker.e164_mobile_network_area_code().map(Value::from)
        }
        "zerooneoneprefix" => |faker, _| faker.zero_one_one_prefix().map(Value::from),
        "zeroonefourprefix" => |faker, _| faker.zero_one_four_prefix().map(Value::from),
        "zeroonefiveprefix" => |faker, _| faker.zero_one_five_prefix().map(Value::from),
        "onetoeight" => |faker, _| faker.one_to_eight().map(Value::from),
        "zerotoeight" => |faker, _| faker.zero_to_eight().map(Value::from),
        "phonenumber06" => |faker, _| faker.phone_number06().map(Value::from),
        "phonenumber06withseparator" => {
            |faker, _| faker.phone_number06_with_separator().map(Value::from)
        }
        "phonenumber07" => |faker, _| faker.phone_number07().map(Value::from),
        "phonenumber07withseparator" => {
            |faker, _| faker.phone_number07_with_separator().map(Value::from)
        }
        "phonenumber08" => |faker, _| faker.phone_number08().map(Value::from),
        "phonenumber08withseparator" => {
            |faker, _| faker.phone_number08_with_separator().map(Value::from)
        }
        "faxnumber" => |faker, _| faker.fax_number().map(Value::from),
        "localareaphonenumber" => |faker, _| faker.local_area_phone_number().map(Value::from),
        "tollfreenumber" => |faker, _| faker.toll_free_number().map(Value::from),
        "personalnumber" => |faker, _| faker.personal_number().map(Value::from),
        "sharedcostnumber" => |faker, _| faker.shared_cost_number().map(Value::from),
        "premiumratenumber" => |faker, _| faker.premium_rate_number().map(Value::from),
        "premiumratephonenumber" => |faker, _| faker.premium_rate_phone_number().map(Value::from),
        "premiumphonenumber" => |faker, _| faker.premium_phone_number().map(Value::from),
        "tollfreeinternationalnumber" => {
            |faker, _| faker.toll_free_international_number().map(Value::from)
        }
        "tollfreelinenumber" => |faker, _| faker.toll_free_line_number().map(Value::from),
        "servicenumber" => |faker, _| faker.service_number().map(Value::from),
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
