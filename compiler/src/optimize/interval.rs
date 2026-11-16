/*!
 * Intervals of integers: the facts the optimizer proves about values.
 *
 * Lymra's integers have at most 64 bits, so every value fits in `i128`, and
 * so does the result of one operation on two of them, except a product of two
 * large `unsigned64` values. That product, and bounds that grow with loop trip
 * counts, saturate at the ends of `i128`. A saturated interval is never inside
 * an integer type, so it never proves anything false.
 */

use crate::ir::{ArithmeticOperator, BitOperator, IntegerType};

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

    /**
     * `next`, with each end that moved past `self` taken to that end of
     * `whole`. A bound that keeps moving gets there in one step, so a fixed
     * point that adds iteration after iteration is reached in a few rounds.
     */
    #[must_use]
    pub fn widen(self, next: Self, whole: Self) -> Self {
        Self {
            low: if next.low < self.low {
                whole.low
            } else {
                next.low
            },
            high: if next.high > self.high {
                whole.high
            } else {
                next.high
            },
        }
    }

    /// Every square of a value in the interval. Unlike a product of two values taken apart, it is never negative.
    #[must_use]
    pub fn square(self) -> Self {
        let (low, high) = (
            self.low.saturating_mul(self.low),
            self.high.saturating_mul(self.high),
        );
        if self.low >= 0 {
            Self { low, high }
        } else if self.high <= 0 {
            Self {
                low: high,
                high: low,
            }
        } else {
            Self {
                low: 0,
                high: low.max(high),
            }
        }
    }

    /**
     * The values whose square lies in `squares`, within `self`. A value
     * whose square is at least the low end lies on either side of zero, so
     * that bound only narrows an interval that stays on one side. `None` when
     * no value of `self` has such a square.
     */
    pub fn square_roots(self, squares: Self) -> Option<Self> {
        if squares.high < 0 {
            return None;
        }
        let largest = squares.high.isqrt();
        let mut roots = self.intersect(Self {
            low: -largest,
            high: largest,
        })?;
        if squares.low > 0 {
            let floor = squares.low.isqrt();
            let smallest = if floor * floor < squares.low {
                floor + 1
            } else {
                floor
            };
            if roots.low >= 0 {
                roots = roots.intersect(Self {
                    low: smallest,
                    high: i128::MAX,
                })?;
            } else if roots.high <= 0 {
                roots = roots.intersect(Self {
                    low: i128::MIN,
                    high: -smallest,
                })?;
            }
        }
        Some(roots)
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

    /**
     * The values `operator` gives for a left value in this interval, of type
     * `ty`, and a right one in `other`: the other value whose bits combine
     * with it, or the count of a shift. Each bound follows from how two's
     * complement stores a value, the same for every width, and where none
     * applies the result can be any value of the type.
     */
    pub fn bits(self, operator: BitOperator, other: Self, ty: IntegerType) -> Self {
        let bounded = match operator {
            BitOperator::And => self.and(other, ty),
            BitOperator::Or => self.or(other),
            BitOperator::Xor => self.xor(other, ty),
            BitOperator::ShiftLeft | BitOperator::ShiftRight => {
                // A count outside the bits of the type stops the program, so only the others give a value.
                let counts = other.intersect(Self {
                    low: 0,
                    high: i128::from(ty.bits()) - 1,
                });
                counts.and_then(|counts| match operator {
                    BitOperator::ShiftLeft => self.shift_left(counts, ty),
                    _ => Some(self.shift_right(counts)),
                })
            }
        };
        bounded.unwrap_or(Self::of_type(ty))
    }

    /**
     * Clearing bits lowers a value of either sign, and a value that is not
     * negative keeps a clear sign bit: so `x & y` lies from 0 up to `x` when
     * `x` is not negative, and at most at the lower of two negative values.
     */
    fn and(self, other: Self, ty: IntegerType) -> Option<Self> {
        match (Sign::of(self), Sign::of(other)) {
            (Sign::NotNegative, Sign::NotNegative) => Some(Self {
                low: 0,
                high: self.high.min(other.high),
            }),
            (Sign::NotNegative, _) => Some(Self {
                low: 0,
                high: self.high,
            }),
            (_, Sign::NotNegative) => Some(Self {
                low: 0,
                high: other.high,
            }),
            (Sign::Negative, Sign::Negative) => Some(Self {
                low: ty.minimum(),
                high: self.high.min(other.high),
            }),
            _ => None,
        }
    }

    /**
     * Setting bits raises a value of either sign, so `x | y` is at least the
     * higher of the two. Two values that are not negative set no bit above
     * the highest either has, and a negative value keeps the result negative.
     */
    fn or(self, other: Self) -> Option<Self> {
        match (Sign::of(self), Sign::of(other)) {
            (Sign::NotNegative, Sign::NotNegative) => Some(Self {
                low: self.low.max(other.low),
                high: all_ones_through(self.high.max(other.high)),
            }),
            (Sign::Negative, Sign::Negative) => Some(Self {
                low: self.low.max(other.low),
                high: -1,
            }),
            (Sign::Negative, Sign::NotNegative) => Some(Self {
                low: self.low,
                high: -1,
            }),
            (Sign::NotNegative, Sign::Negative) => Some(Self {
                low: other.low,
                high: -1,
            }),
            _ => None,
        }
    }

    /**
     * `x ^ y` has no bit above the highest of two values that are not
     * negative, and neither has the result for two negative values, which is
     * the same as for their complements. One of each gives a negative value,
     * the complement of such a result. With every bit set, as `bit_not` has
     * it, the result is the exact complement.
     */
    fn xor(self, other: Self, ty: IntegerType) -> Option<Self> {
        let all_ones = Self::exact(ty.from_bits(u64::MAX));
        if self == all_ones || other == all_ones {
            let value = if self == all_ones { other } else { self };
            return Some(Self {
                low: complement(value.high, ty),
                high: complement(value.low, ty),
            });
        }
        match (Sign::of(self), Sign::of(other)) {
            (Sign::NotNegative, Sign::NotNegative) => Some(Self {
                low: 0,
                high: all_ones_through(self.high.max(other.high)),
            }),
            (Sign::Negative, Sign::Negative) => Some(Self {
                low: 0,
                high: all_ones_through((-1 - self.low).max(-1 - other.low)),
            }),
            (Sign::Negative, Sign::NotNegative) => Some(Self {
                low: -1 - all_ones_through((-1 - self.low).max(other.high)),
                high: -1,
            }),
            (Sign::NotNegative, Sign::Negative) => Some(Self {
                low: -1 - all_ones_through(self.high.max(-1 - other.low)),
                high: -1,
            }),
            _ => None,
        }
    }

    /// A shift up by `counts` multiplies by a power of two, while the product fits the type and so loses no bit.
    fn shift_left(self, counts: Self, ty: IntegerType) -> Option<Self> {
        let powers = Self {
            low: 1 << counts.low,
            high: 1 << counts.high,
        };
        Some(self.multiply(powers)).filter(|product| product.fits(ty))
    }

    /**
     * A shift down by `counts` divides by a power of two, rounding toward
     * minus infinity. It is monotonic in the value, and a larger count moves
     * a value toward 0, or toward -1 for a negative one, so the corners bound
     * it.
     */
    fn shift_right(self, counts: Self) -> Self {
        Self {
            low: (self.low >> counts.low).min(self.low >> counts.high),
            high: (self.high >> counts.low).max(self.high >> counts.high),
        }
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

/// Where the values of an interval lie: all of them not negative, all negative, or some of each.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sign {
    NotNegative,
    Negative,
    Both,
}

impl Sign {
    const fn of(interval: Interval) -> Self {
        if interval.low >= 0 {
            Self::NotNegative
        } else if interval.high < 0 {
            Self::Negative
        } else {
            Self::Both
        }
    }
}

/// The value with every bit set from bit 0 through the highest bit of `value`, which is not negative.
const fn all_ones_through(value: i128) -> i128 {
    if value == 0 {
        0
    } else {
        i128::MAX >> (value.leading_zeros() - 1)
    }
}

/// `value` with every bit of `ty` flipped: `-value - 1` for a signed type, the largest value less `value` for an unsigned one.
const fn complement(value: i128, ty: IntegerType) -> i128 {
    if ty.is_signed() {
        -1 - value
    } else {
        ty.maximum() - value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interval(low: i128, high: i128) -> Interval {
        Interval { low, high }
    }

    /// Values inside `range` worth trying: its ends, the values next to them, and the interesting values inside it.
    fn samples(range: Interval, interesting: &[i128]) -> Vec<i128> {
        let mut values: Vec<i128> = [range.low, range.low + 1, range.high - 1, range.high]
            .into_iter()
            .chain(interesting.iter().copied())
            .filter(|value| range.contains(*value))
            .collect();
        values.sort_unstable();
        values.dedup();
        values
    }

    /**
     * Every result of every bit operation on values of an 8-bit type, taken
     * from many pairs of intervals, lies inside the interval the analysis
     * gives for that pair. A shift is tried with counts outside the bits
     * too, which stop the program and so give no result to bound.
     */
    #[test]
    fn bit_operations_stay_inside_their_bounds() {
        let operators = [
            BitOperator::And,
            BitOperator::Or,
            BitOperator::Xor,
            BitOperator::ShiftLeft,
            BitOperator::ShiftRight,
        ];
        for ty in [IntegerType::Integer8, IntegerType::Unsigned8] {
            let interesting: Vec<i128> = [
                -128, -127, -65, -64, -9, -2, -1, 0, 1, 2, 7, 8, 63, 64, 100, 127, 128, 200, 254,
                255,
            ]
            .into_iter()
            .filter(|value| ty.contains(*value))
            .collect();
            let ranges: Vec<Interval> = interesting
                .iter()
                .flat_map(|&low| {
                    interesting
                        .iter()
                        .filter(move |&&high| high >= low)
                        .map(move |&high| interval(low, high))
                })
                .collect();
            let counts: Vec<Interval> = [(-2, 0), (0, 0), (0, 3), (1, 7), (3, 9), (7, 7), (8, 9)]
                .into_iter()
                .map(|(low, high)| interval(low, high))
                .collect();
            for operator in operators {
                let rights = if operator.is_shift() {
                    &counts
                } else {
                    &ranges
                };
                for &left in &ranges {
                    for &right in rights {
                        let bound = left.bits(operator, right, ty);
                        for x in samples(left, &interesting) {
                            for y in samples(right, &interesting) {
                                if let Some(result) = operator.apply(x, y, ty) {
                                    assert!(
                                        bound.contains(result),
                                        "{operator:?} of {x} in {left:?} and {y} in {right:?} as {ty:?} gives {result}, outside {bound:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn bit_operations_bound_what_masks_and_shifts_keep() {
        let any = Interval::of_type(IntegerType::Integer64);
        // A mask that is not negative bounds any value it is applied to.
        assert_eq!(
            any.bits(BitOperator::And, interval(255, 255), IntegerType::Integer64),
            interval(0, 255)
        );
        // `bit_not` is `bit_xor` with every bit set, and flips the bounds exactly.
        assert_eq!(
            interval(-3, 10).bits(BitOperator::Xor, interval(-1, -1), IntegerType::Integer64),
            interval(-11, 2)
        );
        assert_eq!(
            interval(0, 10).bits(BitOperator::Xor, interval(255, 255), IntegerType::Unsigned8),
            interval(245, 255)
        );
        // A shift down by a known count divides, and a shift up that cannot lose a bit multiplies.
        assert_eq!(
            any.bits(
                BitOperator::ShiftRight,
                interval(56, 56),
                IntegerType::Integer64
            ),
            interval(-128, 127)
        );
        assert_eq!(
            interval(0, 255).bits(
                BitOperator::ShiftLeft,
                interval(8, 8),
                IntegerType::Integer64
            ),
            interval(0, 65280)
        );
        // A shift up that can push a bit out of the type could give any value of it.
        assert_eq!(
            interval(0, 255).bits(
                BitOperator::ShiftLeft,
                interval(1, 1),
                IntegerType::Unsigned8
            ),
            Interval::of_type(IntegerType::Unsigned8)
        );
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
    fn squares_and_their_roots_stay_exact() {
        assert_eq!(interval(-3, 5).square(), interval(0, 25));
        assert_eq!(interval(-5, -2).square(), interval(4, 25));
        assert_eq!(interval(2, 4).square(), interval(4, 16));
        let squares = |low, high| interval(low, high);
        // `divisor * divisor <= 3_000_000` keeps the divisor at most 1732.
        assert_eq!(
            interval(2, i128::from(i64::MAX)).square_roots(squares(i128::MIN, 3_000_000)),
            Some(interval(2, 1732))
        );
        assert_eq!(
            interval(-100, 100).square_roots(squares(0, 16)),
            Some(interval(-4, 4))
        );
        // At least 10 needs a root of at least 4 on the side the interval keeps to.
        assert_eq!(
            interval(0, 100).square_roots(squares(10, i128::MAX)),
            Some(interval(4, 100))
        );
        assert_eq!(
            interval(-100, 0).square_roots(squares(9, i128::MAX)),
            Some(interval(-100, -3))
        );
        // Both sides of zero stay possible, so a low end cannot narrow.
        assert_eq!(
            interval(-100, 100).square_roots(squares(9, i128::MAX)),
            Some(interval(-100, 100))
        );
        assert_eq!(interval(0, 100).square_roots(squares(i128::MIN, -1)), None);
        assert_eq!(interval(0, 3).square_roots(squares(10, 20)), None);
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
