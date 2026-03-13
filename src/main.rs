use std::fs;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, ExitCode};

use clap::{Parser, Subcommand};

const PF_ANCHOR_NAME: &str = "focus";
const PF_ANCHOR_PATH: &str = "/etc/pf.anchors/focus";
const PF_CONF_PATH: &str = "/etc/pf.conf";
const PF_TABLE_NAME: &str = "focus_blocked";

#[derive(Subcommand, Debug, Clone)]
enum Command {
    /// Block one or more domains
    Block { domains: Vec<String> },
    /// Unblock one or more domains
    Unblock { domains: Vec<String> },
    /// List currently blocked domains
    List,
    /// Re-resolve all domains and reload firewall rules
    Refresh,
}

#[derive(Parser)]
#[command(about = "Block distracting websites using macOS pf firewall")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Block { domains } => cmd_block(&domains),
        Command::Unblock { domains } => cmd_unblock(&domains),
        Command::List => cmd_list(),
        Command::Refresh => cmd_refresh(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

// -- Config / state -----------------------------------------------------------

fn config_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".config").join("focus"))
}

fn read_domains() -> Result<Vec<String>, String> {
    let path = config_dir()?.join("domains.txt");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let domains: Vec<String> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    Ok(domains)
}

fn write_domains(domains: &[String]) -> Result<(), String> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let content = if domains.is_empty() {
        String::new()
    } else {
        domains.join("\n") + "\n"
    };
    let path = dir.join("domains.txt");
    fs::write(&path, content)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))
}

// -- DNS resolution -----------------------------------------------------------

fn resolve_host(host: &str, ips: &mut Vec<IpAddr>) {
    // Query multiple times — CDNs return different IPs per query
    for _ in 0..5 {
        let Ok(addrs) = (host, 443).to_socket_addrs() else {
            break;
        };
        for addr in addrs {
            if !ips.contains(&addr.ip()) {
                ips.push(addr.ip());
            }
        }
    }
    // Also try dig for IPs that ToSocketAddrs misses
    for record in ["A", "AAAA"] {
        let Ok(output) = ProcessCommand::new("dig")
            .args(["+short", host, record])
            .output()
        else {
            continue;
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Ok(ip) = line.trim().parse::<IpAddr>()
                && !ips.contains(&ip)
            {
                ips.push(ip);
            }
        }
    }
}

fn resolve_domain(domain: &str) -> Vec<IpAddr> {
    let mut ips = Vec::new();
    for host in [domain.to_string(), format!("www.{domain}")] {
        resolve_host(&host, &mut ips);
    }
    ips
}

fn resolve_all_domains(domains: &[String]) -> Result<Vec<IpAddr>, String> {
    let mut all_ips = Vec::new();
    let mut any_resolved = false;

    for domain in domains {
        let ips = resolve_domain(domain);
        if ips.is_empty() {
            eprintln!("warning: {domain} -- failed to resolve, skipping");
        } else {
            println!("{domain} -> {} IPs", ips.len());
            any_resolved = true;
            for ip in ips {
                if !all_ips.contains(&ip) {
                    all_ips.push(ip);
                }
            }
        }
    }

    if !any_resolved && !domains.is_empty() {
        return Err(
            "could not resolve any domains -- check your network connection".to_string(),
        );
    }
    Ok(all_ips)
}

// -- pf management ------------------------------------------------------------

fn generate_anchor_content(ips: &[IpAddr]) -> String {
    if ips.is_empty() {
        return String::new();
    }
    let ip_list: Vec<String> = ips.iter().map(ToString::to_string).collect();
    format!(
        "table <{PF_TABLE_NAME}> persist {{ {} }}\n\
         block return out quick from any to <{PF_TABLE_NAME}>\n",
        ip_list.join(", ")
    )
}

fn run_pfctl(args: &[&str]) -> Result<String, String> {
    let output = ProcessCommand::new("pfctl")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run pfctl: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // pfctl writes informational messages to stderr even on success,
    // so we only treat non-zero exit as an error.
    if !output.status.success() {
        let detail = if stderr.is_empty() { &stdout } else { &stderr };
        return Err(format!("pfctl {} failed: {detail}", args.join(" ")));
    }
    Ok(stdout)
}

fn write_anchor_file(content: &str) -> Result<(), String> {
    fs::write(PF_ANCHOR_PATH, content).map_err(|e| {
        format!("failed to write {PF_ANCHOR_PATH}: {e} -- run with sudo")
    })
}

fn ensure_anchor_in_pf_conf() -> Result<bool, String> {
    let conf = fs::read_to_string(PF_CONF_PATH)
        .map_err(|e| format!("failed to read {PF_CONF_PATH}: {e}"))?;

    let anchor_ref = format!("anchor \"{PF_ANCHOR_NAME}\"");
    if conf.lines().any(|line| line.trim() == anchor_ref) {
        return Ok(false);
    }

    let load_line = format!(
        "load anchor \"{PF_ANCHOR_NAME}\" from \"{PF_ANCHOR_PATH}\""
    );

    // Insert after the last existing anchor/load line for correct evaluation order
    let mut lines: Vec<&str> = conf.lines().collect();
    let last_anchor_idx = lines
        .iter()
        .rposition(|l| l.trim().starts_with("anchor ") || l.trim().starts_with("load anchor "));

    let insert_at = last_anchor_idx.map_or(lines.len(), |i| i + 1);
    lines.insert(insert_at, &anchor_ref);
    lines.insert(insert_at + 1, &load_line);

    let new_conf = lines.join("\n") + "\n";
    fs::write(PF_CONF_PATH, new_conf).map_err(|e| {
        format!("failed to write {PF_CONF_PATH}: {e} -- run with sudo")
    })?;
    Ok(true)
}

fn apply_rules(domains: &[String]) -> Result<(), String> {
    let anchor_added = ensure_anchor_in_pf_conf()?;

    if domains.is_empty() {
        write_anchor_file("")?;
        run_pfctl(&["-a", PF_ANCHOR_NAME, "-F", "all"])?;
        println!("all rules cleared");
        return Ok(());
    }

    let ips = resolve_all_domains(domains)?;
    println!("blocking {} IPs total", ips.len());

    let content = generate_anchor_content(&ips);
    write_anchor_file(&content)?;

    // If we just added the anchor to pf.conf, reload the main ruleset
    // so pf recognizes the new anchor evaluation point
    if anchor_added {
        run_pfctl(&["-f", PF_CONF_PATH])?;
    }

    run_pfctl(&["-a", PF_ANCHOR_NAME, "-f", PF_ANCHOR_PATH])?;
    run_pfctl(&["-E"])?;

    Ok(())
}

// -- Command handlers ---------------------------------------------------------

fn cmd_block(new_domains: &[String]) -> Result<(), String> {
    if new_domains.is_empty() {
        return Err("provide at least one domain to block".to_string());
    }
    for domain in new_domains {
        if !domain.contains('.') || domain.contains(' ') {
            return Err(format!(
                "invalid domain: '{domain}' -- provide bare domain like instagram.com"
            ));
        }
    }

    let mut current = read_domains()?;
    for domain in new_domains {
        if !current.contains(domain) {
            current.push(domain.clone());
        }
    }
    current.sort();
    write_domains(&current)?;
    apply_rules(&current)?;
    print_domains(&current);
    Ok(())
}

fn cmd_unblock(remove_domains: &[String]) -> Result<(), String> {
    if remove_domains.is_empty() {
        return Err("provide at least one domain to unblock".to_string());
    }

    let mut current = read_domains()?;
    current.retain(|d| !remove_domains.contains(d));
    write_domains(&current)?;
    apply_rules(&current)?;
    print_domains(&current);
    Ok(())
}

fn cmd_list() -> Result<(), String> {
    let domains = read_domains()?;
    print_domains(&domains);
    Ok(())
}

fn cmd_refresh() -> Result<(), String> {
    let domains = read_domains()?;
    if domains.is_empty() {
        println!("no domains to refresh");
        return Ok(());
    }
    apply_rules(&domains)?;
    println!("firewall rules refreshed");
    Ok(())
}

fn print_domains(domains: &[String]) {
    if domains.is_empty() {
        println!("no domains blocked");
        return;
    }
    for domain in domains {
        println!("{domain}");
    }
}
