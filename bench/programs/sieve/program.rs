fn main() {
    let mut composite: Vec<bool> = Vec::new();
    for _ in 0..100_000_000 {
        composite.push(false);
    }
    let mut count: i64 = 0;
    let mut candidate: usize = 2;
    while candidate < composite.len() {
        if !composite[candidate] {
            count += 1;
            let mut multiple = candidate * candidate;
            while multiple < composite.len() {
                composite[multiple] = true;
                multiple += candidate;
            }
        }
        candidate += 1;
    }
    println!("{count}");
}
