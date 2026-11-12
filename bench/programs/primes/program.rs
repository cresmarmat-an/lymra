fn is_prime(candidate: i64) -> bool {
    if candidate < 2 {
        return false;
    }
    let mut divisor = 2;
    while divisor * divisor <= candidate {
        if candidate % divisor == 0 {
            return false;
        }
        divisor += 1;
    }
    true
}

fn main() {
    let count = (0..3_000_000_i64).filter(|&candidate| is_prime(candidate)).count();
    println!("{count}");
}
