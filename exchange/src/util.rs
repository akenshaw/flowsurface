use serde::{Deserialize, Serialize};

pub type ContractSize = Power10<-4, 6>;
pub type MinTicksize = Power10<-8, 2>;
pub type MinQtySize = Power10<-6, 8>;

#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
pub struct Power10<const MIN: i8, const MAX: i8> {
    pub power: i8,
}

impl<const MIN: i8, const MAX: i8> Power10<MIN, MAX> {
    #[inline]
    pub fn new(power: i8) -> Self {
        Self {
            power: power.clamp(MIN, MAX),
        }
    }

    #[inline]
    pub fn as_f32(self) -> f32 {
        10f32.powi(self.power as i32)
    }
}

impl<const MIN: i8, const MAX: i8> From<Power10<MIN, MAX>> for f32 {
    fn from(v: Power10<MIN, MAX>) -> Self {
        v.as_f32()
    }
}

impl<const MIN: i8, const MAX: i8> From<f32> for Power10<MIN, MAX> {
    fn from(value: f32) -> Self {
        if value <= 0.0 {
            return Self { power: 0 };
        }
        let log10 = value.abs().log10();
        let rounded = log10.round() as i8;
        let power = rounded.clamp(MIN, MAX);
        Self { power }
    }
}

impl<const MIN: i8, const MAX: i8> serde::Serialize for Power10<MIN, MAX> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // serialize as a plain numeric (e.g. 0.1, 1, 10)
        let v: f32 = (*self).into();
        serializer.serialize_f32(v)
    }
}

impl<'de, const MIN: i8, const MAX: i8> serde::Deserialize<'de> for Power10<MIN, MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = f32::deserialize(deserializer)?;
        Ok(Self::from(v))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct PriceStep {
    /// step size in atomic units (10^-PRICE_SCALE)
    pub units: i64,
}

impl PriceStep {
    /// Lossy: from f32 step (rounds to nearest atomic unit)
    pub fn from_f32_lossy(step: f32) -> Self {
        assert!(step > 0.0, "step must be > 0");
        let scale = 10f32.powi(Price::PRICE_SCALE);
        let units = (step * scale).round() as i64;
        assert!(units > 0, "step too small at given PRICE_SCALE");
        Self { units }
    }

    pub fn from_f32(step: f32) -> Self {
        Self::from_f32_lossy(step)
    }
}

/// Fixed atomic unit scale: 10^-PRICE_SCALE is the smallest stored fraction.
/// MinTicksize has range [-8, 2], e.g. PRICE_SCALE = 8 to represent 10^-8 atomic units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Deserialize, Serialize)]
pub struct Price {
    /// number of atomic units (atomic unit = 10^-PRICE_SCALE)
    pub units: i64,
}

impl Price {
    /// number of decimal places of the atomic unit (10^-8)
    pub const PRICE_SCALE: i32 = 8;

    pub fn to_string_dp(self, dp: u32) -> String {
        let dp = dp.min(Self::PRICE_SCALE as u32);
        let scale = Self::PRICE_SCALE as u32;
        let unit = 10i64.pow(scale - dp);

        let u = self.units;
        let half = unit / 2;
        let rounded_units = if u >= 0 {
            ((u + half).div_euclid(unit)) * unit
        } else {
            ((u - half).div_euclid(unit)) * unit
        };

        let sign = if rounded_units < 0 { "-" } else { "" };
        let abs_u = (rounded_units as i128).unsigned_abs();

        let scale_pow = 10u128.pow(scale);
        let int_part = abs_u / scale_pow;
        if dp == 0 {
            return format!("{}{}", sign, int_part);
        }

        let frac_div = 10u128.pow(scale - dp);
        let frac_part = (abs_u % scale_pow) / frac_div;
        let frac_str = format!("{:0width$}", frac_part, width = dp as usize);

        format!("{}{}.{frac_str}", sign, int_part)
    }

    /// Lossy: convert price to f32, may lose precision if going beyond `PRICE_SCALE`
    pub fn to_f32_lossy(self) -> f32 {
        let scale = 10f32.powi(Self::PRICE_SCALE);
        (self.units as f32) / scale
    }

    /// Lossy: create Price from f32 (rounds to nearest atomic unit)
    pub fn from_f32_lossy(v: f32) -> Self {
        let scale = 10f32.powi(Self::PRICE_SCALE);
        let u = (v * scale).round() as i64;
        Self { units: u }
    }

    pub fn from_f32(v: f32) -> Self {
        Self::from_f32_lossy(v)
    }

    pub fn to_f32(self) -> f32 {
        self.to_f32_lossy()
    }

    pub fn round_to_step(self, step: PriceStep) -> Self {
        let unit = step.units;
        if unit <= 1 {
            return self;
        }
        let half = unit / 2;
        let rounded = ((self.units + half).div_euclid(unit)) * unit;
        Self { units: rounded }
    }

    /// Floor to multiple of an arbitrary step
    fn floor_to_step(self, step: PriceStep) -> Self {
        let unit = step.units;
        if unit <= 1 {
            return self;
        }
        let floored = (self.units.div_euclid(unit)) * unit;
        Self { units: floored }
    }

    /// Ceil to multiple of an arbitrary step
    fn ceil_to_step(self, step: PriceStep) -> Self {
        let unit = step.units;
        if unit <= 1 {
            return self;
        }
        let added = self.units.checked_add(unit - 1).unwrap_or_else(|| {
            if self.units.is_negative() {
                i64::MIN
            } else {
                i64::MAX
            }
        });

        let ceiled = (added.div_euclid(unit)) * unit;
        Self { units: ceiled }
    }

    /// Group with arbitrary step (e.g. sells floor, buys ceil)
    pub fn round_to_side_step(self, is_sell_or_bid: bool, step: PriceStep) -> Self {
        if is_sell_or_bid {
            self.floor_to_step(step)
        } else {
            self.ceil_to_step(step)
        }
    }

    /// Create Price from raw atomic units (no rounding) — internal only
    pub fn from_units(units: i64) -> Self {
        Self { units }
    }

    /// Returns the atomic-unit count that corresponds to one min tick (min_tick / atomic_unit)
    fn min_tick_units(min_tick: MinTicksize) -> i64 {
        let exp = Self::PRICE_SCALE + (min_tick.power as i32);
        assert!(exp >= 0, "PRICE_SCALE must be >= -min_tick.power");
        10i64
            .checked_pow(exp as u32)
            .expect("min_tick_units overflowed")
    }

    /// Round this Price to the nearest multiple of the provided min_ticksize
    pub fn round_to_min_tick(self, min_tick: MinTicksize) -> Self {
        let unit = Self::min_tick_units(min_tick);
        if unit <= 1 {
            return self;
        }
        let half = unit / 2;
        let rounded = ((self.units + half).div_euclid(unit)) * unit;
        Self { units: rounded }
    }
}

impl std::ops::Add for Price {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            units: self
                .units
                .checked_add(rhs.units)
                .expect("Price add overflowed"),
        }
    }
}

impl std::ops::Div<i64> for Price {
    type Output = Self;

    fn div(self, rhs: i64) -> Self::Output {
        Self {
            units: self.units.div_euclid(rhs),
        }
    }
}

impl std::ops::Sub for Price {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            units: self
                .units
                .checked_sub(rhs.units)
                .expect("Price sub overflowed"),
        }
    }
}

#[cfg(test)]
mod manual_printouts {
    use super::*;

    #[test]
    fn show_min_tick_rounding() {
        let orig: f32 = 0.000051;
        let p = Price::from_f32_lossy(orig);
        let back = p.to_f32_lossy();

        let scale = 10f32.powi(Price::PRICE_SCALE);
        let expected_units = (orig * scale).round() as i64;
        let expected_back = (expected_units as f32) / scale;

        println!("orig (f32)        = {:0.9}", orig);
        println!("orig bits         = 0x{:08x}", orig.to_bits());
        println!("price units       = {}", p.units);
        println!("expected units    = {}", expected_units);
        println!("back (from units) = {:0.9}", back);
        println!("expected back     = {:0.9}", expected_back);
        println!("orig - back       = {:+.9e}", orig - back);
        println!("back == expected  = {}", back == expected_back);
    }
}
