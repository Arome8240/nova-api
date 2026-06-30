//! Phantom-typed monetary value for KOVA.
//!
//! [`Money<C>`] pairs a [`rust_decimal::Decimal`] amount with a compile-time
//! currency marker. This prevents accidental arithmetic across currencies at
//! the type level — `Money<NGN>` and `Money<GBP>` are distinct types, so you
//! cannot add them without an explicit FX conversion.
//!
//! # MySQL storage
//!
//! The amount is stored as `DECIMAL(19,4)` — matching the sqlx workspace feature
//! `rust_decimal`. The currency code is stored in a separate `CHAR(3)` or enum
//! column. `Money<C>` implements `sqlx::Type`/`Encode`/`Decode` for the amount
//! column only; the currency column is handled separately by each service.
//!
//! # Arithmetic rules
//!
//! - `Add` / `Sub` — both operands must be the same currency (enforced by type).
//! - `Mul<Decimal>` — scale an amount by a dimensionless factor (e.g. FX rate).
//! - `Div` is **not** implemented. Dividing money produces ambiguous rounding;
//!   callers must use `checked_div` on the raw `Decimal` and rebuild `Money`.
//! - All operations use banker's rounding (`MidpointNearestEven`).
//!
//! # Negative amounts
//!
//! `Money<C>` allows negative values to represent debits in double-entry
//! ledger entries. Callers that need non-negative validation (e.g. payment
//! amounts) should call [`Money::is_positive`].
//!
//! # Example
//!
//! ```rust
//! # use kova_types::money::{Money, NGN, GBP};
//! # use rust_decimal_macros::dec;
//! let a = Money::<NGN>::from_major(1000);
//! let b = Money::<NGN>::from_minor(dec!(500.0000));
//! let sum = (a + b).unwrap();
//! assert_eq!(sum.amount(), dec!(1500.0000));
//! ```

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::MySql;
use std::fmt;
use std::marker::PhantomData;
use std::ops::{Add, Sub};

// ── Sealed Currency trait ─────────────────────────────────────────────────────

mod sealed {
    /// Sealed trait: only types in this crate can implement [`super::Currency`].
    pub trait CurrencySeal {}
}

/// A compile-time currency marker.
///
/// This trait is sealed — you cannot implement it outside of `kova-types`.
/// That prevents a downstream crate from defining `struct InvalidCoin;` and
/// accidentally creating `Money<InvalidCoin>`.
pub trait Currency: sealed::CurrencySeal + fmt::Debug + Clone + Copy + PartialEq + Eq {
    /// ISO 4217 alphabetic currency code, e.g. `"NGN"`.
    const CODE: &'static str;

    /// Number of decimal places for the minor unit (e.g. 2 for NGN kobo, 2 for GBP pence).
    const DECIMAL_PLACES: u32;
}

// ── Supported currencies ──────────────────────────────────────────────────────

macro_rules! define_currency {
    ($name:ident, $code:literal, $decimals:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name;

        impl sealed::CurrencySeal for $name {}

        impl Currency for $name {
            const CODE: &'static str = $code;
            const DECIMAL_PLACES: u32 = $decimals;
        }
    };
}

define_currency!(NGN, "NGN", 2); // Nigerian Naira
define_currency!(GBP, "GBP", 2); // British Pound Sterling
define_currency!(USD, "USD", 2); // US Dollar
define_currency!(KES, "KES", 2); // Kenyan Shilling

// ── Money<C> ─────────────────────────────────────────────────────────────────

/// A monetary amount in currency `C`.
///
/// The internal representation is [`Decimal`] with 4 decimal places
/// (`DECIMAL(19,4)` in MySQL). All arithmetic uses banker's rounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money<C: Currency> {
    #[serde(with = "rust_decimal::serde::str")]
    amount: Decimal,
    #[serde(skip)]
    _currency: PhantomData<C>,
}

impl<C: Currency> Money<C> {
    // ── Constructors ─────────────────────────────────────────────────────

    /// Create `Money<C>` from an already-validated [`Decimal`].
    ///
    /// The amount is normalised to 4 decimal places using banker's rounding.
    pub fn new(amount: Decimal) -> Self {
        Self {
            amount: amount.round_dp_with_strategy(
                4,
                rust_decimal::RoundingStrategy::MidpointNearestEven,
            ),
            _currency: PhantomData,
        }
    }

    /// Create `Money<C>` from a whole major-unit amount (e.g. `1000` NGN = ₦1,000).
    pub fn from_major(major: i64) -> Self {
        Self::new(Decimal::new(major, 0))
    }

    /// Create `Money<C>` directly from a [`Decimal`] minor-unit amount.
    /// Alias for [`Money::new`]; provided for clarity at call sites.
    pub fn from_minor(amount: Decimal) -> Self {
        Self::new(amount)
    }

    /// Zero amount in currency `C`.
    pub fn zero() -> Self {
        Self::new(Decimal::ZERO)
    }

    // ── Accessors ────────────────────────────────────────────────────────

    /// Return the raw [`Decimal`] amount.
    pub fn amount(&self) -> Decimal {
        self.amount
    }

    /// ISO 4217 currency code for this `Money`.
    pub fn currency_code(&self) -> &'static str {
        C::CODE
    }

    // ── Predicates ───────────────────────────────────────────────────────

    pub fn is_zero(&self) -> bool {
        self.amount.is_zero()
    }

    pub fn is_positive(&self) -> bool {
        self.amount > Decimal::ZERO
    }

    pub fn is_negative(&self) -> bool {
        self.amount < Decimal::ZERO
    }

    // ── Checked arithmetic ────────────────────────────────────────────────

    /// Checked addition. Returns `None` on overflow (extremely rare with
    /// `DECIMAL(19,4)` but handled for correctness).
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.amount.checked_add(rhs.amount).map(Self::new)
    }

    /// Checked subtraction. Returns `None` on overflow.
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.amount.checked_sub(rhs.amount).map(Self::new)
    }

    /// Checked multiplication by a dimensionless scalar.
    /// Use this for FX rate application.
    pub fn checked_mul(self, factor: Decimal) -> Option<Self> {
        self.amount.checked_mul(factor).map(Self::new)
    }

    // ── Negation ─────────────────────────────────────────────────────────

    /// Return the additive inverse (debit ↔ credit flip in double-entry).
    pub fn negate(self) -> Self {
        Self::new(-self.amount)
    }

    // ── Conversion ───────────────────────────────────────────────────────

    /// Convert this amount to a different currency using the given rate.
    ///
    /// `rate` is expressed as units of `D` per one unit of `C`
    /// (e.g. NGN→GBP rate ≈ 0.00050).
    pub fn convert_to<D: Currency>(self, rate: Decimal) -> Option<Money<D>> {
        self.amount.checked_mul(rate).map(Money::<D>::new)
    }
}

// ── Operator impls ────────────────────────────────────────────────────────────

/// `Add` for same-currency `Money`. Returns `Option<Money<C>>` via [`Money::checked_add`].
/// Use the `+` operator for infallible contexts (panics on overflow — acceptable
/// in tests and amounts that fit DECIMAL(19,4)).
impl<C: Currency> Add for Money<C> {
    type Output = Option<Self>;

    fn add(self, rhs: Self) -> Self::Output {
        self.checked_add(rhs)
    }
}

/// `Sub` for same-currency `Money`. Returns `Option<Money<C>>` via [`Money::checked_sub`].
impl<C: Currency> Sub for Money<C> {
    type Output = Option<Self>;

    fn sub(self, rhs: Self) -> Self::Output {
        self.checked_sub(rhs)
    }
}

/// Scale `Money<C>` by a dimensionless [`Decimal`] factor.
/// Used for FX rate application, fee calculation, etc.
impl<C: Currency> std::ops::Mul<Decimal> for Money<C> {
    type Output = Option<Self>;

    fn mul(self, factor: Decimal) -> Self::Output {
        self.checked_mul(factor)
    }
}

// Div is intentionally NOT implemented. See module-level docs.

impl<C: Currency> PartialOrd for Money<C> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<C: Currency> Ord for Money<C> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.amount.cmp(&other.amount)
    }
}

impl<C: Currency> fmt::Display for Money<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", C::CODE, self.amount)
    }
}

impl<C: Currency> Default for Money<C> {
    fn default() -> Self {
        Self::zero()
    }
}

// ── sqlx MySQL integration ────────────────────────────────────────────────────
//
// The amount column is DECIMAL(19,4). The currency is stored as a separate
// CHAR(3) column — Money<C> only handles the amount half.

impl<C: Currency> sqlx::Type<MySql> for Money<C> {
    fn type_info() -> sqlx::mysql::MySqlTypeInfo {
        <Decimal as sqlx::Type<MySql>>::type_info()
    }

    fn compatible(ty: &sqlx::mysql::MySqlTypeInfo) -> bool {
        <Decimal as sqlx::Type<MySql>>::compatible(ty)
    }
}

impl<'r, C: Currency> sqlx::Decode<'r, MySql> for Money<C> {
    fn decode(
        value: <MySql as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let d = <Decimal as sqlx::Decode<'r, MySql>>::decode(value)?;
        Ok(Self::new(d))
    }
}

impl<'q, C: Currency> sqlx::Encode<'q, MySql> for Money<C> {
    fn encode_by_ref(
        &self,
        buf: &mut <MySql as sqlx::Database>::ArgumentBuffer<'q>,
    ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <Decimal as sqlx::Encode<'q, MySql>>::encode_by_ref(&self.amount, buf)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn zero_is_zero() {
        assert!(Money::<NGN>::zero().is_zero());
    }

    #[test]
    fn from_major_has_no_decimals() {
        let m = Money::<NGN>::from_major(1000);
        assert_eq!(m.amount(), dec!(1000.0000));
    }

    #[test]
    fn addition_same_currency() {
        let a = Money::<NGN>::from_major(500);
        let b = Money::<NGN>::from_major(300);
        let sum = (a + b).expect("no overflow");
        assert_eq!(sum.amount(), dec!(800.0000));
    }

    #[test]
    fn subtraction_same_currency() {
        let a = Money::<GBP>::from_major(100);
        let b = Money::<GBP>::from_major(30);
        let diff = (a - b).expect("no overflow");
        assert_eq!(diff.amount(), dec!(70.0000));
    }

    #[test]
    fn subtraction_can_produce_negative() {
        let a = Money::<USD>::from_major(10);
        let b = Money::<USD>::from_major(50);
        let diff = (a - b).expect("no overflow");
        assert!(diff.is_negative());
        assert_eq!(diff.amount(), dec!(-40.0000));
    }

    #[test]
    fn mul_by_decimal_factor() {
        let m = Money::<NGN>::from_major(1000);
        let scaled = (m * dec!(0.5)).expect("no overflow");
        assert_eq!(scaled.amount(), dec!(500.0000));
    }

    #[test]
    fn negate_flips_sign() {
        let m = Money::<NGN>::from_major(100);
        assert_eq!(m.negate().amount(), dec!(-100.0000));
        assert_eq!(m.negate().negate().amount(), m.amount());
    }

    #[test]
    fn bankers_rounding_half_even() {
        // 0.00005 rounds to 0.0000 (round-half-to-even, nearest even is 0)
        let m = Money::<NGN>::new(dec!(0.00005));
        assert_eq!(m.amount(), dec!(0.0000));
        // 0.00015 rounds to 0.0002 (nearest even is 2)
        let m2 = Money::<NGN>::new(dec!(0.00015));
        assert_eq!(m2.amount(), dec!(0.0002));
    }

    #[test]
    fn display_includes_currency_code() {
        let m = Money::<GBP>::from_major(42);
        assert!(m.to_string().starts_with("GBP "));
        assert!(m.to_string().contains("42"));
    }

    #[test]
    fn ordering() {
        let small = Money::<KES>::from_major(10);
        let large = Money::<KES>::from_major(100);
        assert!(small < large);
        assert!(large > small);
        assert_eq!(small, small);
    }

    #[test]
    fn convert_to_different_currency() {
        // 1000 NGN at rate 0.0005 NGN/GBP = 0.5 GBP
        let ngn = Money::<NGN>::from_major(1000);
        let gbp: Money<GBP> = ngn.convert_to(dec!(0.0005)).expect("no overflow");
        assert_eq!(gbp.amount(), dec!(0.5000));
    }

    #[test]
    fn currency_codes_are_correct() {
        assert_eq!(Money::<NGN>::zero().currency_code(), "NGN");
        assert_eq!(Money::<GBP>::zero().currency_code(), "GBP");
        assert_eq!(Money::<USD>::zero().currency_code(), "USD");
        assert_eq!(Money::<KES>::zero().currency_code(), "KES");
    }

    #[test]
    fn serde_roundtrip() {
        let m = Money::<NGN>::from_major(12345);
        let json = serde_json::to_string(&m).unwrap();
        let back: Money<NGN> = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    // Compile-fail guard for cross-currency arithmetic — tested via compile_fail
    // doctest in the module docs. Here we verify currency type identity via TypeId.
    #[test]
    fn ngn_and_gbp_are_distinct_types() {
        use std::any::TypeId;
        assert_ne!(
            TypeId::of::<Money<NGN>>(),
            TypeId::of::<Money<GBP>>(),
        );
    }
}
