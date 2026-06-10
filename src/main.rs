fn main() {
    fez::reset_sigpipe();
    std::process::exit(fez::run(fez::cli::parse()));
}
