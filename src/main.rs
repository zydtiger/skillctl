fn main() {
    if let Err(error) = skillctl::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
