fn main() {
    if let Err(error) = octra_sqlite::run_cli() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
