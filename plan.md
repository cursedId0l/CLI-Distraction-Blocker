Here's the plan:
Goal: Rust CLI that blocks sites at the pf firewall level on macOS

Resolve the target domain to both IPv4 and IPv6 addresses
Write a pf anchor file at /etc/pf.anchors/focus with block rules for those IPs
Shell out to pfctl to load the anchor and enable pf
Handle sudo requirements either by prompting or setting up a sudoers rule for the specific pfctl commands
Build commands like focus block instagram.com and focus unblock instagram.com
Optionally persist the block list so it survives reboots by adding a launchd plist that reloads the pf rules on startup

Things to learn along the way:

pf syntax: tables, anchors, block rules
How pf integrates with macOS specifically since it differs slightly from BSD
std::process::Command in Rust for shelling out
How to handle privilege escalation cleanly in a CLI tool

Should be a fun weekend project and you'll come out knowing firewalls properly.
