fn greatest_common_divisor(mut left: i64, mut right: i64) -> i64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn main() {
    let mut total: i64 = 0;
    for first in 1..2_000_i64 {
        for second in 1..2_000_i64 {
            total += greatest_common_divisor(first, second);
        }
    }
    println!("{total}");
}
