fn fibonacci(number: i64) -> i64 {
    if number < 2 {
        return number;
    }
    fibonacci(number - 1) + fibonacci(number - 2)
}

fn main() {
    println!("{}", fibonacci(40));
}
