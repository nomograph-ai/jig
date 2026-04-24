#![deny(warnings, clippy::all)]

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "jig", version, about = "Agent-shape testing harness")]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
    println!("jig: scaffold. Implementation begins at t3 (schema design).");
}
