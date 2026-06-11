fn main() {
    fez::reset_sigpipe();
    // parse_or_render honors --json for clap usage errors and renders help/
    // version itself; on either it returns the exit code to use directly.
    match fez::cli::parse_or_render() {
        Ok(cli) => std::process::exit(fez::run(cli)),
        Err(code) => std::process::exit(code),
    }
}
