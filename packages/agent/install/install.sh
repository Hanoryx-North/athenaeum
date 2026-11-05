#!/bin/sh
# Installs the Athenaeum agent on macOS and Linux. Nothing needs to be installed first:
#
#   curl -fsSL https://github.com/Hanoryx-North/athenaeum/releases/latest/download/install.sh | sh
#
# It puts a private, checksum-verified Node next to the agent (so it never depends on, or
# disturbs, whatever Node you have), then runs install.mjs, which installs the rest. Extra
# arguments go to install.mjs:  ... | sh -s -- --prefix "$HOME/athenaeum-agent"
set -eu

NODE_VERSION="v22.22.0"
BASE="${ATHENAEUM_AGENT_BASE_URL:-https://github.com/Hanoryx-North/athenaeum/releases/latest/download}"

fail() { printf 'error: %s\n' "$1" >&2; exit 1; }

case "$(uname -s)" in
  Darwin) os=darwin; ext=tar.gz; prefix_default="$HOME/Library/Application Support/Athenaeum/agent" ;;
  Linux)  os=linux;  ext=tar.xz; prefix_default="${XDG_DATA_HOME:-$HOME/.local/share}/athenaeum/agent" ;;
  *) fail "this installer is for macOS and Linux; on Windows use install.ps1" ;;
esac
case "$(uname -m)" in
  x86_64|amd64)  arch=x64 ;;
  arm64|aarch64) arch=arm64 ;;
  *) fail "unsupported CPU: $(uname -m)" ;;
esac

# Node's published SHA-256 for each archive, pinned here so a tampered download is refused.
case "$os-$arch" in
  darwin-arm64) sha=5ed4db0fcf1eaf84d91ad12462631d73bf4576c1377e192d222e48026a902640 ;;
  darwin-x64)   sha=5ea50c9d6dea3dfa3abb66b2656f7a4e1c8cef23432b558d45fb538c7b5dedce ;;
  linux-arm64)  sha=1bf1eb9ee63ffc4e5d324c0b9b62cf4a289f44332dfef9607cea1a0d9596ba6f ;;
  linux-x64)    sha=9aa8e9d2298ab68c600bd6fb86a6c13bce11a4eca1ba9b39d79fa021755d7c37 ;;
esac

# Honour --prefix so the private Node lands beside the rest of the install.
prefix="$prefix_default"
prev=""
for arg in "$@"; do
  if [ "$prev" = "--prefix" ]; then prefix="$arg"; fi
  prev="$arg"
done

command -v curl >/dev/null 2>&1 || fail "curl is required"
if command -v sha256sum >/dev/null 2>&1; then
  hash_of() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  hash_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  fail "sha256sum or shasum is required to verify the download"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

node_dir="$prefix/node"
if [ -x "$node_dir/bin/node" ] && [ "$("$node_dir/bin/node" -v)" = "$NODE_VERSION" ]; then
  echo "Node $NODE_VERSION is already in place."
else
  name="node-$NODE_VERSION-$os-$arch"
  echo "Fetching Node $NODE_VERSION (kept private to the agent)..."
  curl -fsSL "https://nodejs.org/dist/$NODE_VERSION/$name.$ext" -o "$work/node.$ext"
  [ "$(hash_of "$work/node.$ext")" = "$sha" ] || fail "Node download failed its checksum; refusing to use it"
  mkdir -p "$work/node-extract"
  tar -xf "$work/node.$ext" -C "$work/node-extract"
  mkdir -p "$prefix"
  rm -rf "$node_dir"
  mv "$work/node-extract/$name" "$node_dir"
fi

echo "Fetching the installer..."
curl -fsSL "$BASE/agent-install.mjs" -o "$work/install.mjs"
curl -fsSL "$BASE/pins.json" -o "$work/pins.json"

# Not `exec`: the EXIT trap must still remove the temporary directory.
"$node_dir/bin/node" "$work/install.mjs" --prefix "$prefix" "$@"
