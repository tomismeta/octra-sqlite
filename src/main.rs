fn main() {
    if let Err(error) = octra_sqlite::cli::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
