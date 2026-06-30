/// Implement `sqlx::Type`, `sqlx::Encode`, `sqlx::Decode`, `Display`, and
/// `FromStr` for an enum that maps to a MySQL string column (VARCHAR or ENUM).
///
/// # Usage
/// ```ignore
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// pub enum MyCurrency { NGN, GBP }
///
/// impl_sqlx_string_enum!(MyCurrency,
///     NGN => "NGN",
///     GBP => "GBP",
/// );
/// ```
macro_rules! impl_sqlx_string_enum {
    ($name:ty, $($variant:ident => $str:literal),+ $(,)?) => {
        impl sqlx::Type<sqlx::MySql> for $name {
            fn type_info() -> sqlx::mysql::MySqlTypeInfo {
                <String as sqlx::Type<sqlx::MySql>>::type_info()
            }
            fn compatible(ty: &sqlx::mysql::MySqlTypeInfo) -> bool {
                <String as sqlx::Type<sqlx::MySql>>::compatible(ty)
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::MySql> for $name {
            fn decode(
                value: <sqlx::MySql as sqlx::Database>::ValueRef<'r>,
            ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
                let s = <String as sqlx::Decode<'r, sqlx::MySql>>::decode(value)?;
                match s.as_str() {
                    $($str => Ok(<$name>::$variant),)+
                    other => Err(format!(
                        "unknown {} value: {}",
                        stringify!($name),
                        other
                    ).into()),
                }
            }
        }

        impl<'q> sqlx::Encode<'q, sqlx::MySql> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut <sqlx::MySql as sqlx::Database>::ArgumentBuffer<'q>,
            ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
                let s = self.to_string();
                <String as sqlx::Encode<'q, sqlx::MySql>>::encode_by_ref(&s, buf)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let s: &'static str = match self {
                    $(<$name>::$variant => $str,)+
                };
                f.write_str(s)
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($str => Ok(<$name>::$variant),)+
                    other => Err(format!(
                        "unknown {} value: {}",
                        stringify!($name),
                        other
                    )),
                }
            }
        }
    };
}
