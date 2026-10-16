#!/bin/sh
# Install mcp-router from GitHub Releases.
#   curl -fsSL https://raw.githubusercontent.com/rabin-a/mcp-router/master/install.sh | sh
# Options (environment): MCP_ROUTER_VERSION=v0.1.0  MCP_ROUTER_INSTALL_DIR=$HOME/.local/bin
set -eu

REPO="rabin-a/mcp-router"
VERSION="${MCP_ROUTER_VERSION:-latest}"
INSTALL_DIR="${MCP_ROUTER_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-gnu" ;;
  *) echo "mcp-router: unsupported OS $(uname -s); build from source with cargo install --git https://github.com/$REPO" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) arch="aarch64" ;;
  x86_64 | amd64) arch="x86_64" ;;
  *) echo "mcp-router: unsupported architecture $(uname -m)" >&2; exit 1 ;;
esac
target="$arch-$os"

if [ "$VERSION" = "latest" ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "Downloading mcp-router ($target, $VERSION)..."
curl -fsSL "$base/mcp-router-$target.tar.gz" -o "$tmp/mcp-router.tar.gz"
curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"

expected="$(grep "mcp-router-$target.tar.gz" "$tmp/SHA256SUMS" | cut -d' ' -f1)"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/mcp-router.tar.gz" | cut -d' ' -f1)"
else
  actual="$(shasum -a 256 "$tmp/mcp-router.tar.gz" | cut -d' ' -f1)"
fi
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  echo "mcp-router: checksum mismatch, refusing to install" >&2
  exit 1
fi

tar xzf "$tmp/mcp-router.tar.gz" -C "$tmp"
mkdir -p "$INSTALL_DIR"
install -m 755 "$tmp/mcp-router-$target/mcp-router" "$INSTALL_DIR/mcp-router"
echo "Installed $INSTALL_DIR/mcp-router"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Note: $INSTALL_DIR is not on your PATH. Add it, e.g.: export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac
echo "Next: create ~/.config/mcp-router/servers.toml, then run: claude mcp add router -- mcp-router"
