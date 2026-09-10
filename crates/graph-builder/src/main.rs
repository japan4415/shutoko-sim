//! CLI entry point for shutoko-graph-builder.
//! Full CLI functionality will be extended in subsequent pipeline steps.

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && (args[1] == "--version" || args[1] == "-V") {
        println!("shutoko-graph-builder {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    println!("shutoko-graph-builder: offline OSM to directed road graph pipeline");
    println!("Usage: shutoko-graph-builder [--help] [--version]");
}
