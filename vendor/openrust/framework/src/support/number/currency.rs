use super::{NumberError, NumberValue, round, special_value};
use icu_decimal::provider::{Baked as DecimalData, DecimalDigitsV1, DecimalSymbolsV1};
use icu_experimental::{
    dimension::{
        currency::{CurrencyType, formatter::CurrencyFormatter},
        provider::currency::{
            essentials::CurrencyEssentialsV1,
            fractions::{CurrencyFractions, CurrencyFractionsV1, FractionInfo},
            no_currency::CurrencyPatternsNoCurrencyV1,
            symbols::{CurrencyDecimalSymbolsV1, CurrencySymbolsV1},
        },
    },
    provider::Baked as CurrencyData,
};
use icu_locale_core::Locale;
use icu_provider::prelude::*;

pub(super) fn format(
    locale: &Locale,
    number: NumberValue,
    currency: CurrencyType,
    precision: Option<u8>,
) -> Result<String, NumberError> {
    if special_value(number).is_some() {
        return Ok(intl::number::format_currency(
            &locale.to_string(),
            number.as_float(),
            currency.iso_code().as_str(),
        ));
    }
    let fractions: DataResponse<CurrencyFractionsV1> = CurrencyData.load(Default::default())?;
    let digits = precision.unwrap_or_else(|| fractions.payload.get().resolve(currency).digits);
    let rounded = round(number.decimal()?, Some(digits), None, digits);
    let prefs = locale.clone().into();
    let formatter = if let Some(precision) = precision {
        CurrencyFormatter::try_new_symbol_unstable(
            &Precision(precision),
            prefs,
            currency,
            Default::default(),
        )?
    } else {
        CurrencyFormatter::try_new_symbol(prefs, currency, Default::default())?
    };
    Ok(formatter.format_fixed_decimal(&rounded).to_string())
}

// ICU4X's public currency options use the CLDR fraction count. Laravel
// accepts an override, so supply that one datum with the same CLDR patterns.
struct Precision(u8);

impl DataProvider<CurrencyFractionsV1> for Precision {
    fn load(&self, _: DataRequest) -> Result<DataResponse<CurrencyFractionsV1>, DataError> {
        Ok(DataResponse {
            metadata: Default::default(),
            payload: DataPayload::from_owned(CurrencyFractions {
                fractions: Default::default(),
                default: FractionInfo {
                    digits: self.0,
                    ..Default::default()
                },
            }),
        })
    }
}

macro_rules! delegate_data {
    ($provider:ident: $($marker:ident),+ $(,)?) => {
        $(
            impl DataProvider<$marker> for Precision {
                fn load(&self, request: DataRequest) -> Result<DataResponse<$marker>, DataError> {
                    $provider.load(request)
                }
            }
        )+
    };
}

delegate_data!(CurrencyData: CurrencyEssentialsV1, CurrencyPatternsNoCurrencyV1,
    CurrencyDecimalSymbolsV1, CurrencySymbolsV1);
delegate_data!(DecimalData: DecimalDigitsV1, DecimalSymbolsV1);
