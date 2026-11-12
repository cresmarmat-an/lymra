fn main() {
    let mut total: i64 = 0;
    for index in 0..300_000_000_i64 {
        total = total + index % 7 * 3;
    }
    println!("{total}");
}
