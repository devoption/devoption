fn main() {
    let mut buffer = itoa::Buffer::new();
    println!("verified seed: {}", buffer.format(42));
}
