fn main() {
    use clap::Parser;
    std::process::exit(fez::run(fez::cli::Cli::parse()));
}
