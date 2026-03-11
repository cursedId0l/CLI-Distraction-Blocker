use clap::{Parser, Subcommand};
use std::fs;

#[derive(Subcommand, Debug, Clone)]
enum Command {
    Block { domains: Vec<String> },
    Unblock { domains: Vec<String> },
    List,
}
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

fn main() {
    let cli = Cli::parse();
    println!("{:?}", cli.command);
}

fn read_hosts_file() -> String {
    fs::read_to_string("/etc/hosts").expect("read_hosts_file:: Unable to read host file")
}
