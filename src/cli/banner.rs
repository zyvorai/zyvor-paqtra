// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

use owo_colors::OwoColorize;

/// Print the interlocking Paqtra banner (Cilium-inspired).
pub fn print_banner() {
    println!("{}", "    /¯¯\\".yellow());
    println!("{}{}{}", " /¯¯".cyan(), "\\__/".yellow(), "¯¯\\".green());
    println!("{}{}{}", " \\__".cyan(), "/¯¯\\".red(), "__/".green());
    println!("{}{}{}", " /¯¯".green(), "\\__/".red(), "¯¯\\".magenta());
    println!("{}{}{}", " \\__".green(), "/¯¯\\".blue(), "__/".magenta());
    println!("{}", "    \\__/".blue());
    println!();
}

pub fn print_root_help() {
    print_banner();
    println!(
        "{}",
        "Paqtra — trace every flow. Network observability for Kubernetes.".bold()
    );
    println!();
    println!("CLI to install, manage, and observe Paqtra on Kubernetes (Cilium + Hubble).");
    println!();
    println!("{}", "Examples:".bold());
    println!();
    println!("  Install Paqtra in the current Kubernetes context");
    println!();
    println!("    $ {}", "paqtra install".cyan());
    println!();
    println!("  Check status");
    println!();
    println!("    $ {}", "paqtra status".cyan());
    println!();
    println!("  Open the interactive TUI");
    println!();
    println!("    $ {}", "paqtra tui".cyan());
    println!();
    println!("{}", "Usage:".bold());
    println!("  paqtra [command]");
    println!();
    println!("{}", "Available Commands:".bold());
    println!("  agent       Run the node agent (DaemonSet)");
    println!("  ebpf        Read-only BPF attachments / drift");
    println!("  features    Feature discovery catalog");
    println!("  help        Help about any command");
    println!("  info        Show cluster / install info");
    println!("  install     Install Paqtra using Helm");
    println!("  status      Display status");
    println!("  tui         Launch the interactive TUI");
    println!("  uninstall   Uninstall Paqtra using Helm");
    println!("  upgrade     Upgrade a Paqtra installation");
    println!("  version     Display version information");
    println!();
    println!("Use \"paqtra [command] --help\" for more information about a command.");
}
