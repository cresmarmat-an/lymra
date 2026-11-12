fn main() {
    let mut total: i64 = 0;
    for round in 0..20_i64 {
        for index in 0..50_000_000_i64 {
            total = total + index * 3 + round;
        }
    }
    println!("{total}");
}
