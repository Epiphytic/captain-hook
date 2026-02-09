#!/usr/bin/env bash
set -euo pipefail

# captain-hook installer
# Builds the Rust binary and sets up the project for use as a Claude Code plugin.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "captain-hook installer"
echo "======================"
echo ""

# Check for Rust toolchain
if ! command -v cargo &>/dev/null; then
	echo "Error: cargo is not installed."
	echo "Install Rust via https://rustup.rs/ and try again."
	exit 1
fi

# Build the release binary
echo "Building captain-hook (release)..."
cd "$PROJECT_ROOT"
cargo build --release

BINARY="$PROJECT_ROOT/target/release/captain-hook"
if [ ! -f "$BINARY" ]; then
	echo "Error: Build failed. Binary not found at $BINARY"
	exit 1
fi

echo "Binary built: $BINARY"
echo ""

# Create global config directory
GLOBAL_CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/captain-hook"
mkdir -p "$GLOBAL_CONFIG_DIR"

if [ ! -f "$GLOBAL_CONFIG_DIR/config.yml" ]; then
	cat >"$GLOBAL_CONFIG_DIR/config.yml" <<'YAML'
# captain-hook global configuration
# See: https://github.com/epiphytic/captain-hook

supervisor:
  backend: socket

# Anthropic API key (if using API supervisor backend).
# Can also be set via ANTHROPIC_API_KEY env var.
# api_key: null

# Embedding model for fastembed. Default: BAAI/bge-small-en-v1.5
# embedding_model: null
YAML
	echo "Created global config: $GLOBAL_CONFIG_DIR/config.yml"
fi

# Initialize .captain-hook/ in the current repo if not present
if [ ! -d "$PROJECT_ROOT/.captain-hook" ]; then
	mkdir -p "$PROJECT_ROOT/.captain-hook/rules"
	echo "Created .captain-hook/ directory"
fi

# Copy template files if they don't exist
if [ ! -f "$PROJECT_ROOT/.captain-hook/policy.yml" ]; then
	if [ -f "$PROJECT_ROOT/.captain-hook/policy.yml.template" ]; then
		cp "$PROJECT_ROOT/.captain-hook/policy.yml.template" "$PROJECT_ROOT/.captain-hook/policy.yml"
	fi
	echo "Note: Create .captain-hook/policy.yml from the template if not already present."
fi

if [ ! -f "$PROJECT_ROOT/.captain-hook/roles.yml" ]; then
	if [ -f "$PROJECT_ROOT/.captain-hook/roles.yml.template" ]; then
		cp "$PROJECT_ROOT/.captain-hook/roles.yml.template" "$PROJECT_ROOT/.captain-hook/roles.yml"
	fi
	echo "Note: Create .captain-hook/roles.yml from the template if not already present."
fi

# Ensure .gitkeep in rules
touch "$PROJECT_ROOT/.captain-hook/rules/.gitkeep"

# Ensure .gitignore for derived artifacts
if [ ! -f "$PROJECT_ROOT/.captain-hook/.gitignore" ]; then
	cat >"$PROJECT_ROOT/.captain-hook/.gitignore" <<'GITIGNORE'
# Derived artifacts - rebuild locally with `captain-hook build`
.index/

# Personal preferences - not shared
.user/
GITIGNORE
	echo "Created .captain-hook/.gitignore"
fi

echo ""
echo "Installation complete."
echo ""
echo "Next steps:"
echo "  1. Add captain-hook to your PATH or use the full binary path"
echo "  2. Run 'captain-hook init' in your project to set up .captain-hook/"
echo "  3. Configure hooks in .claude/settings.json or use the plugin"
echo ""
echo "Quick start:"
echo "  captain-hook register --session-id \$SESSION_ID --role coder"
echo "  captain-hook stats"
echo ""
