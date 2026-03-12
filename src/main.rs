use clap::{Parser, Subcommand};
use std::fs;

const BLOCK_START: &str = "# focus:start";
const BLOCK_END: &str = "# focus:end";

fn host_entries_for_domain(domain: &str) -> Vec<String> {
    vec![
        format!("127.0.0.1 {domain}"),
        format!("127.0.0.1 www.{domain}"),
        format!("::1 {domain}"),
        format!("::1 www.{domain}"),
    ]
}

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
    match cli.command {
        Command::Block { domains } => block_domains(domains),
        Command::Unblock { domains } => unblock_domains(domains),
        Command::List => list_domains(),
    }
}

fn read_hosts_file() -> String {
    fs::read_to_string("/etc/hosts").expect("read_hosts_file:: Unable to read host file")
}

fn parse_blocked_domains(contents: &str) -> Vec<String> {
    let mut domains: Vec<String> = Vec::new();
    let mut inside_block = false;

    for line in contents.lines() {
        if line == BLOCK_START {
            inside_block = true;
        } else if line == BLOCK_END {
            break;
        } else if inside_block {
            let Some(domain) = line.split_whitespace().last() else {
                continue;
            };
            let base = domain.strip_prefix("www.").unwrap_or(domain);
            if !domains.iter().any(|d| d == base) {
                domains.push(base.to_string());
            }
        }
    }

    domains
}

fn list_domains() {
    let hosts_file = read_hosts_file();
    let domains = parse_blocked_domains(&hosts_file);
    for domain in domains {
        println!("{domain}");
    }
}

fn rebuild_hosts_for_block(contents: &str, domains: &[String]) -> String {
    let mut result: Vec<String> = Vec::new();
    let mut found_block = false;

    for line in contents.lines() {
        if line == BLOCK_START {
            found_block = true;
            result.push(line.to_string());
        } else if line == BLOCK_END {
            for domain in domains {
                result.extend(host_entries_for_domain(domain));
            }
            result.push(BLOCK_END.to_string());
        } else if found_block && !result.contains(&BLOCK_END.to_string()) {
            // skip old entries inside the block (they get rewritten above)
            continue;
        } else {
            result.push(line.to_string());
        }
    }

    if !found_block {
        result.push(String::new());
        result.push(BLOCK_START.to_string());
        for domain in domains {
            result.extend(host_entries_for_domain(domain));
        }
        result.push(BLOCK_END.to_string());
    }

    result.join("\n") + "\n"
}

fn rebuild_hosts_for_unblock(contents: &str, domains: &[String]) -> String {
    let mut result: Vec<String> = Vec::new();
    let mut inside_block = false;

    for line in contents.lines() {
        if line == BLOCK_START {
            inside_block = true;
            result.push(line.to_string());
        } else if line == BLOCK_END {
            inside_block = false;
            result.push(line.to_string());
        } else {
            if inside_block {
                if let Some(entry_domain) = line.split_whitespace().last() {
                    let base = entry_domain.strip_prefix("www.").unwrap_or(entry_domain);
                    if domains.iter().any(|d| d == base) {
                        continue;
                    }
                }
            }
            result.push(line.to_string());
        }
    }

    result.join("\n") + "\n"
}

fn block_domains(domains: Vec<String>) {
    let hosts_file = read_hosts_file();
    let new_contents = rebuild_hosts_for_block(&hosts_file, &domains);
    fs::write("/etc/hosts", new_contents).expect("could not write /etc/hosts");
    list_domains()
}
fn unblock_domains(domains: Vec<String>) {
    let hosts_file = read_hosts_file();
    let new_contents = rebuild_hosts_for_unblock(&hosts_file, &domains);
    fs::write("/etc/hosts", new_contents).expect("could not write /etc/hosts");
    list_domains()
}
