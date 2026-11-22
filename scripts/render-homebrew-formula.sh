#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

# Render the Homebrew formula for a release from its sha256sums.txt.
#   scripts/render-homebrew-formula.sh <version> <sha256sums.txt> [repo]
# Prints Formula/paqtra.rb on stdout. Fails if a platform's checksum is missing
# (a formula with a blank sha256 would install nothing useful).
set -euo pipefail

VERSION="${1:?usage: $0 <version> <sha256sums.txt> [owner/repo]}"
SUMS="${2:?usage: $0 <version> <sha256sums.txt> [owner/repo]}"
REPO="${3:-zyvorai/zyvor-paqtra}"
VERSION="${VERSION#v}"

sum_for() {
    local asset="paqtra-${VERSION}-$1.tar.gz" sha
    sha=$(awk -v f="$asset" '$2 == f || $2 == "*" f {print $1}' "$SUMS")
    [ -n "$sha" ] || { echo "no checksum for $asset in $SUMS" >&2; exit 1; }
    printf '%s' "$sha"
}
url_for() { printf 'https://github.com/%s/releases/download/v%s/paqtra-%s-%s.tar.gz' "$REPO" "$VERSION" "$VERSION" "$1"; }

MAC_ARM=$(sum_for macos-aarch64); MAC_X86=$(sum_for macos-x86_64)
LIN_ARM=$(sum_for linux-aarch64); LIN_X86=$(sum_for linux-x86_64)

cat <<RUBY
class Paqtra < Formula
  desc "Cilium-native network observability and operations for Kubernetes"
  homepage "https://github.com/${REPO}"
  version "${VERSION}"
  license :cannot_represent

  on_macos do
    if Hardware::CPU.arm?
      url "$(url_for macos-aarch64)"
      sha256 "${MAC_ARM}"
    else
      url "$(url_for macos-x86_64)"
      sha256 "${MAC_X86}"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "$(url_for linux-aarch64)"
      sha256 "${LIN_ARM}"
    else
      url "$(url_for linux-x86_64)"
      sha256 "${LIN_X86}"
    end
  end

  def install
    bin.install "paqtra"
    generate_completions_from_executable(bin/"paqtra", "completion", shells: [:bash, :zsh, :fish])
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/paqtra --version")
  end
end
RUBY
