# Automatic Cilium Installation & Upgrade Guide

> This page describes the **TUI's** interactive bootstrap: the flags below belong to
> `paqtra tui` (e.g. `paqtra tui --auto-install`), not to bare `paqtra`. To install
> Paqtra itself, or Cilium with Hubble from the CLI, use `paqtra install`
> (`--with-cilium`); see [cli.md](cli.md).

## Overview

Paqtra now supports **automatic installation and upgrade** of Cilium, making it truly zero-touch from cluster to observability.

## Features

### 1. Automatic Installation

If Cilium is not detected in your cluster, the TUI can automatically install it for you.

### 2. Automatic Upgrade

Keep your Cilium installation up-to-date with automatic upgrade capability.

### 3. Interactive or Silent Mode

- **Interactive**: Prompts before installing (default)
- **Silent**: Installs automatically with `--auto-install` flag

## Usage

### Standard Mode (Interactive)

```bash
# Run normally - will prompt if Cilium not found
paqtra tui
```

**Output:**
```
🚀 Bootstrapping Paqtra...
✔ Cluster detected: minikube
🔍 Cilium not detected in the cluster.

❓ Would you like to install Cilium now? (y/N)
```

Type `y` and press Enter to install.

### Automatic Installation (No Prompts)

```bash
# Automatically install if missing, no prompts
paqtra tui --auto-install
```

**Output:**
```
🚀 Bootstrapping Paqtra...
✔ Cluster detected: minikube
🔍 Cilium not detected in the cluster.
📦 Installing Cilium...
⏳ Waiting for Cilium to be ready...
✅ Cilium installed successfully!
✔ Hubble enabled
```

### Automatic Upgrade

```bash
# Upgrade Cilium to latest version before starting
paqtra tui --auto-upgrade
```

**Output:**
```
🚀 Bootstrapping Paqtra...
✔ Cluster detected: minikube
✔ Cilium detected
  Version: v1.14.5
🔄 Auto-upgrade enabled, checking for updates...
✅ Cilium upgraded successfully!
```

### Combined Mode

```bash
# Install if missing, upgrade if present
paqtra tui --auto-install --auto-upgrade
```

## Prerequisites

### Cilium CLI Required

The automatic installation/upgrade features require the `cilium` CLI tool to be installed.

**Check if installed:**
```bash
cilium version
```

**Install Cilium CLI:**

#### Linux
```bash
curl -L --remote-name-all https://github.com/cilium/cilium-cli/releases/latest/download/cilium-linux-amd64.tar.gz
sudo tar xzvfC cilium-linux-amd64.tar.gz /usr/local/bin
rm cilium-linux-amd64.tar.gz
```

#### macOS
```bash
brew install cilium-cli
```

#### Windows
```powershell
curl -LO https://github.com/cilium/cilium-cli/releases/latest/download/cilium-windows-amd64.tar.gz
tar -xf cilium-windows-amd64.tar.gz
# Move cilium.exe to your PATH
```

## How It Works

### Detection Flow

```
Start TUI
    ↓
Detect Cluster ✓
    ↓
Check for Cilium
    ↓
  Found?
   ↙  ↘
 Yes   No
  ↓     ↓
Show    Auto-install?
Version    ↙  ↘
  ↓      Yes  No
Upgrade?  ↓    ↓
  ↙  ↘   Install  Prompt
Yes  No     ↓       ↓
 ↓    ↓    Wait   y/n?
Run  Run  Ready    ↓
TUI  TUI   ↓   Yes ↙ ↘ No
      └────┴──────┘    Exit
            ↓
         Run TUI
```

### Installation Process

When auto-install is triggered:

1. **Check Cilium CLI availability**
   - If missing, show install instructions and exit

2. **Execute `cilium install`**
   - Installs Cilium with default settings
   - Uses Helm or manifest-based install

3. **Wait for Cilium to be ready**
   - Executes `cilium status --wait`
   - Waits for all Cilium pods to be running

4. **Enable Hubble**
   - Configures Hubble observability
   - Sets up required features

5. **Continue bootstrap**
   - Apply network policies
   - Setup RBAC
   - Start TUI

### Upgrade Process

When auto-upgrade is triggered:

1. **Check current version**
   - Queries `cilium version`

2. **Execute `cilium upgrade`**
   - Upgrades to latest available version
   - Preserves existing configuration

3. **Wait for completion**
   - Ensures upgrade completes successfully

4. **Continue bootstrap**
   - Verify new version
   - Continue with TUI launch

## Error Handling

### Cilium CLI Not Found

```
❌ Cilium CLI not found. Please install it first:

Linux:   curl -L --remote-name-all https://github.com/cilium/cilium-cli/releases/latest/download/cilium-linux-amd64.tar.gz
macOS:   brew install cilium-cli

Or visit: https://docs.cilium.io/en/stable/gettingstarted/k8s-install-default/
```

### Installation Failed

```
📦 Installing Cilium...
❌ Failed to install Cilium. Please check the error messages above.
```

Common causes:
- Insufficient cluster resources
- Network connectivity issues
- Incompatible Kubernetes version
- RBAC permissions

### Upgrade Failed

```
🔄 Upgrading Cilium...
❌ Failed to upgrade Cilium
```

Common causes:
- Breaking changes in new version
- Configuration incompatibilities
- Running workloads blocking upgrade

## Use Cases

### 1. Fresh Cluster Setup

Perfect for setting up Cilium on a brand new cluster:

```bash
# Create cluster
minikube start

# Install Cilium + TUI in one command
paqtra tui --auto-install
```

### 2. CI/CD Pipeline

Use in automated pipelines:

```bash
# Non-interactive installation
paqtra tui --auto-install --skip-bootstrap

# Or ensure latest version
paqtra tui --auto-install --auto-upgrade
```

### 3. Development Environment

Quick setup for development:

```bash
# Create test cluster with Cilium
kind create cluster
paqtra tui --auto-install

# Later, ensure up-to-date
paqtra tui --auto-upgrade
```

### 4. Production Clusters

Interactive mode for production (safer):

```bash
# Prompts before making changes
paqtra tui

# Or manual control
cilium install
paqtra tui --skip-bootstrap
```

## Configuration Options

### Install Options

The automatic installation uses Cilium defaults. For custom installation:

```bash
# Install with custom options first
cilium install --set key=value

# Then run TUI
paqtra tui
```

### Upgrade Options

Upgrades use `cilium upgrade` defaults. For custom upgrades:

```bash
# Upgrade with custom options
cilium upgrade --set key=value

# Then run TUI
paqtra tui
```

## Safety Features

### 1. Confirmation Prompts

By default, installation requires user confirmation:
```
❓ Would you like to install Cilium now? (y/N)
```

### 2. CLI Availability Check

Verifies Cilium CLI is installed before attempting operations.

### 3. Status Validation

Waits for Cilium to be fully ready before proceeding:
```
⏳ Waiting for Cilium to be ready...
```

### 4. Error Reporting

Clear error messages for troubleshooting.

## Comparison: Manual vs Auto

| Aspect | Manual | Auto-Install | Auto-Upgrade |
|--------|--------|-------------|--------------|
| Setup Steps | 3-4 | 1 | 1 |
| User Input | Multiple | None* | None |
| Time | 5-10 min | 3-5 min | 2-3 min |
| Error Prone | Medium | Low | Low |
| Flexibility | High | Medium | Low |

*With `--auto-install` flag, otherwise one prompt

## Troubleshooting

### Issue: "Cilium CLI not found"

**Solution:**
```bash
# Install Cilium CLI first
brew install cilium-cli  # macOS
# or download from GitHub releases
```

### Issue: Installation hangs

**Check:**
```bash
# In another terminal
kubectl get pods -n kube-system
cilium status
```

**Solution:**
- Ensure cluster has sufficient resources
- Check network connectivity
- Review Cilium logs: `kubectl logs -n kube-system -l k8s-app=cilium`

### Issue: Upgrade fails

**Solution:**
```bash
# Check current status
cilium status

# Review release notes
cilium upgrade --help

# Manual upgrade with debug
cilium upgrade --debug
```

### Issue: Permission denied

**Solution:**
```bash
# Ensure kubectl has admin access
kubectl auth can-i create clusterroles --all-namespaces

# Use correct kubeconfig
export KUBECONFIG=/path/to/admin/kubeconfig
```

## Best Practices

1. **Test in Dev First**
   - Try auto-install in development before production
   - Verify Cilium version compatibility

2. **Use Interactive Mode in Production**
   - Default (no `--auto-install`) prompts for safety
   - Review changes before applying

3. **Check Cilium CLI Version**
   - Keep Cilium CLI up-to-date
   - `cilium version --client`

4. **Monitor Installation**
   - Watch cluster resources during install
   - Check pod status: `kubectl get pods -n kube-system -w`

5. **Backup Before Upgrade**
   - Backup Cilium config before upgrading
   - Review release notes for breaking changes

## Examples

### Example 1: Complete Fresh Setup

```bash
# Start with empty cluster
minikube start --network-plugin=cni --cni=false

# One command to full observability
paqtra tui --auto-install

# Result:
# ✔ Cilium installed
# ✔ Hubble enabled
# ✔ Policies applied
# ✔ TUI running
```

### Example 2: Ensure Latest Version

```bash
# Upgrade if needed, then run
paqtra tui --auto-upgrade

# Or combined
paqtra tui --auto-install --auto-upgrade
```

### Example 3: CI/CD Integration

```bash
#!/bin/bash
# setup-monitoring.sh

# Create cluster
kind create cluster --name ci-test

# Setup Cilium + monitoring (non-interactive)
paqtra tui --auto-install --skip-bootstrap &

# Run tests
./run-tests.sh

# Cleanup
kind delete cluster --name ci-test
```

## Summary

The automatic installation and upgrade features make Paqtra truly **zero-touch**:

✅ **No manual Cilium installation required**
✅ **One command from empty cluster to full observability**
✅ **Automatic version management**
✅ **CI/CD friendly**
✅ **Safe defaults with interactive confirmations**

Just run `paqtra tui --auto-install` and go!
