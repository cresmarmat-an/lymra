fn chain_length(start: i64) -> i64 {
    let mut value = start;
    let mut steps = 1;
    while value != 1 {
        value = if value % 2 == 0 { value / 2 } else { value * 3 + 1 };
        steps += 1;
    }
    steps
}

fn main() {
    let mut longest = 0;
    let mut best_start = 0;
    for start in 1..1_000_000_i64 {
        let length = chain_length(start);
        if length > longest {
            longest = length;
            best_start = start;
        }
    }
    println!("{best_start} {longest}");
}
