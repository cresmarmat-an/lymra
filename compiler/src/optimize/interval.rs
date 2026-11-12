/*!
 * Intervals of integers: the facts the optimizer proves about values.
 *
 * Lymra's integers have at most 64 bits, so every value fits in `i128`, and
 * so does the result of one operation on two of them, except a product of two
 * large `unsigned64` values. That product, and bounds that grow with loop trip
 * counts, saturate at the ends of `i128`. A saturated interval is never inside
 * an integer type, so it never proves anything false.
 */

use crate::ir::{ArithmeticOperator, IntegerType};

/// The integers from `low` to `high`, both included. Never empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Interval {
    pub low: i128,
    pub high: i128,
}

impl Interval {
    pub const fn exact(value: i128) -> Self {
        Self {
            low: value,
            high: value,
        }
    }

    /// The interval from `first` to `second`, in whichever order they come.
    pub fn spanning(first: i128, second: i128) -> Self {
        Self {
            low: first.min(second),
            high: first.max(second),
        }
    }

    /// Every value of the type.
    pub const fn of_type(ty: IntegerType) -> Self {
        Self {
            low: ty.minimum(),
            high: ty.maximum(),
        }
    }

    /// The smallest interval that holds both.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self {
            low: self.low.min(other.low),
            high: self.high.max(other.high),
        }
    }

    /// The values in both, or `None` when they share none.
    pub fn intersect(self, other: Self) -> Option<Self> {
        let low = self.low.max(other.low);
        let high = self.high.min(other.high);
        (low <= high).then_some(Self { low, high })
    }

    /// Narrows to the type. Every value a checked operation produces lies in its type.
    #[must_use]
    pub fn clamp(self, ty: IntegerType) -> Self {
        self.intersect(Self::of_type(ty))
            .unwrap_or_else(|| Self::of_type(ty))
    }

    pub fn fits(self, ty: IntegerType) -> bool {
        ty.contains(self.low) && ty.contains(self.high)
    }

    pub fn contains(self, value: i128) -> bool {
        self.low <= value && value <= self.high
    }

    #[must_use]
    pub fn negate(self) -> Self {
        Self {
            low: self.high.saturating_neg(),
            high: self.low.saturating_neg(),
        }
    }

    /// Every sum of a value in `self` and a value in `other`.
    #[must_use]
    pub fn add(self, other: Self) -> Self {
        Self {
            low: self.low.saturating_add(other.low),
            high: self.high.saturating_add(other.high),
        }
    }

    /// The interval scaled by a count of at least zero, as for a change repeated `count` times.
    #[must_use]
    pub fn repeat(self, count: i128) -> Self {
        Self::spanning(
            self.low.saturating_mul(count),
            self.high.saturating_mul(count),
        )
        .join(Self::exact(0))
    }

    /**
     * Every result of `operator` on values from the two intervals, computed
     * exactly as C and Lymra do: division truncates toward zero and a
     * remainder takes the sign of the dividend. Returns `None` when a divisor
     * of zero is possible, since then there is no result to bound.
     */
    pub fn apply(self, operator: ArithmeticOperator, other: Self) -> Option<Self> {
        match operator {
            ArithmeticOperator::Add => Some(self.add(other)),
            ArithmeticOperator::Subtract => Some(self.add(other.negate())),
            ArithmeticOperator::Multiply => Some(self.multiply(other)),
            ArithmeticOperator::Divide => self.divide(other),
            ArithmeticOperator::Remainder => self.remainder(other),
        }
    }

    fn multiply(self, other: Self) -> Self {
        let products = [
            self.low.saturating_mul(other.low),
            self.low.saturating_mul(other.high),
            self.high.saturating_mul(other.low),
            self.high.saturating_mul(other.high),
        ];
        Self {
            low: products.iter().copied().min().unwrap_or(i128::MIN),
            high: products.iter().copied().max().unwrap_or(i128::MAX),
        }
    }

    /// Truncating division is monotonic in each operand while the divisor keeps its sign, so the corners bound it.
    fn divide(self, other: Self) -> Option<Self> {
        if other.contains(0) {
            return None;
        }
        let quotients = [
            self.low.checked_div(other.low),
            self.low.checked_div(other.high),
            self.high.checked_div(other.low),
            self.high.checked_div(other.high),
        ];
        // `checked_div` only fails for i128::MIN / -1, which saturates.
        let quotients = quotients.map(|quotient| quotient.unwrap_or(i128::MAX));
        Some(Self {
            low: quotients.iter().copied().min()?,
            high: quotients.iter().copied().max()?,
        })
    }

    /// A remainder is smaller in size than the divisor and has the sign of the dividend.
    fn remainder(self, other: Self) -> Option<Self> {
        if other.contains(0) {
            return None;
        }
        let largest_divisor = other.low.unsigned_abs().max(other.high.unsigned_abs());
        let limit = i128::try_from(largest_divisor - 1).unwrap_or(i128::MAX);
        let low = if self.low < 0 {
            (-limit).max(self.low)
        } else {
            0
        };
        let high = if self.high > 0 {
            limit.min(self.high)
        } else {
            0
        };
        Some(Self { low, high })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interval(low: i128, high: i128) -> Interval {
        Interval { low, high }
    }

    #[test]
    fn arithmetic_is_exact_at_the_corners() {
        let operands = interval(-3, 5);
        assert_eq!(
            operands.apply(ArithmeticOperator::Add, interval(1, 2)),
            Some(interval(-2, 7))
        );
        assert_eq!(
            operands.apply(ArithmeticOperator::Subtract, interval(1, 2)),
            Some(interval(-5, 4))
        );
        assert_eq!(
            operands.apply(ArithmeticOperator::Multiply, interval(-2, 3)),
            Some(interval(-10, 15))
        );
    }

    #[test]
    fn division_needs_a_divisor_that_cannot_be_zero() {
        assert_eq!(
            interval(-7, 7).apply(ArithmeticOperator::Divide, interval(-1, 1)),
            None
        );
        assert_eq!(
            interval(-7, 7).apply(ArithmeticOperator::Divide, interval(2, 3)),
            Some(interval(-3, 3))
        );
        assert_eq!(
            interval(-7, -7).apply(ArithmeticOperator::Divide, interval(-2, -2)),
            Some(interval(3, 3))
        );
    }

    fn remainder(left: Interval, right: Interval) -> Option<Interval> {
        left.apply(ArithmeticOperator::Remainder, right)
    }

    #[test]
    fn remainders_follow_the_sign_of_the_dividend() {
        assert_eq!(
            remainder(interval(0, 100), interval(7, 7)),
            Some(interval(0, 6))
        );
        assert_eq!(remainder(interval(-100, 100), interval(-7, 7)), None);
        assert_eq!(
            remainder(interval(-100, -1), interval(1, 7)),
            Some(interval(-6, 0))
        );
        assert_eq!(
            remainder(interval(0, 3), interval(10, 10)),
            Some(interval(0, 3))
        );
    }

    #[test]
    fn every_result_of_every_operator_lies_in_the_computed_interval() {
        let operators = [
            ArithmeticOperator::Add,
            ArithmeticOperator::Subtract,
            ArithmeticOperator::Multiply,
            ArithmeticOperator::Divide,
            ArithmeticOperator::Remainder,
        ];
        let intervals = [
            interval(-9, -2),
            interval(-4, 4),
            interval(0, 0),
            interval(1, 1),
            interval(3, 11),
        ];
        for operator in operators {
            for left in intervals {
                for right in intervals {
                    let Some(bounds) = left.apply(operator, right) else {
                        continue;
                    };
                    for a in left.low..=left.high {
                        for b in right.low..=right.high {
                            let result = match operator {
                                ArithmeticOperator::Add => a + b,
                                ArithmeticOperator::Subtract => a - b,
                                ArithmeticOperator::Multiply => a * b,
                                ArithmeticOperator::Divide => a / b,
                                ArithmeticOperator::Remainder => a % b,
                            };
                            assert!(
                                bounds.contains(result),
                                "{a} {operator:?} {b} = {result} is outside {bounds:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn repetition_includes_doing_nothing() {
        assert_eq!(interval(2, 5).repeat(10), interval(0, 50));
        assert_eq!(interval(-1, 3).repeat(4), interval(-4, 12));
        assert_eq!(interval(-1, 3).repeat(0), interval(0, 0));
    }

    #[test]
    fn saturated_bounds_never_fit_a_type() {
        let huge = interval(i128::MAX - 1, i128::MAX).add(interval(10, 10));
        assert!(!huge.fits(IntegerType::Unsigned64));
        assert!(interval(0, 255).fits(IntegerType::Unsigned8));
        assert!(!interval(-1, 255).fits(IntegerType::Unsigned8));
    }
}
