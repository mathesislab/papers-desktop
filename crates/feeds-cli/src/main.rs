//! `feeds`: the program of the feeds crate, on the system's environment, clock and network.

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = feeds_cli::Host::system();
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    let code = feeds_cli::run(args, &host, &mut out, &mut err);
    let _ = out.flush();
    std::process::exit(code);
}
