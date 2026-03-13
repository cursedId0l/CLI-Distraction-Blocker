# Focus

A Rust CLI that blocks distracting websites at the macOS pf (packet filter) firewall level.

## How It Works

1. You tell focus which domains to block (e.g. `instagram.com`, `x.com`)
2. Focus resolves each domain (+ its `www.` variant) to IP addresses via DNS
3. It writes a pf anchor file at `/etc/pf.anchors/focus` with a table of those IPs and a block rule
4. It loads the anchor into the macOS firewall via `pfctl` and enables pf
5. Any outbound connection to those IPs gets an immediate TCP RST (connection refused) — the browser fails fast instead of hanging

## Setup

One-time setup happens automatically on first `block` command:

1. Focus adds `anchor "focus"` and a corresponding `load anchor` line to `/etc/pf.conf`
2. It reloads the main pf ruleset so macOS recognizes the new anchor
3. It creates `~/.config/focus/` for the domain state file

After that, the anchor reference persists across reboots (it's in pf.conf), and the `load anchor` directive reloads the rules on boot.

### Install

```
cargo install --path .
```

## Usage

All commands require `sudo` (firewall operations need root):

```bash
# Block sites
sudo focus block instagram.com x.com

# See what's blocked
sudo focus list

# Re-resolve DNS and reload rules (CDN IPs rotate)
sudo focus refresh

# Unblock
sudo focus unblock instagram.com
```

## Files

| Path | Purpose |
|------|---------|
| `~/.config/focus/domains.txt` | Persisted list of blocked domains (one per line) |
| `/etc/pf.anchors/focus` | Generated pf rules (IP table + block rule) |
| `/etc/pf.conf` | System pf config (focus adds its anchor reference here once) |

## Architecture

```
focus block instagram.com
  |
  v
read ~/.config/focus/domains.txt      (existing blocked domains)
  |
  v
merge new domains, dedup, sort
  |
  v
write updated domains.txt
  |
  v
resolve all domains -> IP addresses   (std::net::ToSocketAddrs)
  |
  v
generate pf anchor content:
  table <focus_blocked> persist { 31.13.80.174, 172.66.0.227, ... }
  block return out quick from any to <focus_blocked>
  |
  v
write /etc/pf.anchors/focus
  |
  v
pfctl -a focus -f /etc/pf.anchors/focus   (load anchor rules)
pfctl -E                                    (enable pf, ref-counted)
```

### Why `block return` not `block drop`

- `block drop` silently discards packets — browser hangs for 30+ seconds waiting for a timeout
- `block return` sends TCP RST — browser gets instant "connection refused" (2ms)

### Why pf not /etc/hosts

- `/etc/hosts` only affects DNS lookups that respect it — browsers with DNS-over-HTTPS bypass it
- CDN-heavy sites (Instagram, X) resolve to many IPs that rotate constantly
- pf operates at the network layer — blocks actual TCP connections regardless of how DNS was resolved

### Limitations

- IP-based blocking captures a snapshot of where domains resolve *right now*
- CDN IPs rotate, so `refresh` should be run periodically
- Blocking the main domain IPs is enough to make sites unusable (API calls fail even if cached HTML renders)
