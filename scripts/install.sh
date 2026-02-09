#!/usr/bin/env bash
set -euo pipefail

# captain-hook installer
# Downloads the captain-hook binary and sets up the Claude Code plugin.
# https://github.com/Epiphytic/captain-hook

VERSION=""
INSTALL_DIR="$HOME/.local/bin"
SCOPE="user"
SKIP_PLUGIN=false
LOCAL_BINARY=""
REPO="Epiphytic/captain-hook"
PLUGIN_DIR="$HOME/.captain-hook/plugin"

# ---------- Colors and output helpers ----------

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

info() { printf "${BLUE}[info]${NC}  %s\n" "$*"; }
success() { printf "${GREEN}[ok]${NC}    %s\n" "$*"; }
warn() { printf "${YELLOW}[warn]${NC}  %s\n" "$*"; }
error() { printf "${RED}[error]${NC} %s\n" "$*" >&2; }
fatal() {
	error "$@"
	exit 1
}
step() { printf "\n${BOLD}==> %s${NC}\n" "$*"; }

# ---------- Usage ----------

usage() {
	cat <<'EOF'
captain-hook installer

Downloads the captain-hook binary from GitHub releases and sets up the
Claude Code plugin for permission gating.

USAGE:
    install.sh [OPTIONS]

OPTIONS:
    --version VERSION     Install a specific version (default: latest release)
    --install-dir DIR     Binary install directory (default: ~/.local/bin)
    --scope SCOPE         Plugin scope: user|project|local (default: user)
    --skip-plugin         Only install binary, skip plugin setup
    --binary PATH         Use a local binary instead of downloading
    -h, --help            Show this help message

EXAMPLES:
    # Install latest version
    curl -fsSL https://raw.githubusercontent.com/Epiphytic/captain-hook/main/scripts/install.sh | bash

    # Install a specific version
    ./scripts/install.sh --version 0.1.0

    # Install binary only, custom location
    ./scripts/install.sh --install-dir /usr/local/bin --skip-plugin

    # Use a pre-built binary (CI/testing)
    ./scripts/install.sh --binary ./target/release/captain-hook
EOF
	exit 0
}

# ---------- Argument parsing ----------

while [[ $# -gt 0 ]]; do
	case "$1" in
	--version)
		VERSION="$2"
		shift 2
		;;
	--install-dir)
		INSTALL_DIR="$2"
		shift 2
		;;
	--scope)
		SCOPE="$2"
		shift 2
		;;
	--skip-plugin)
		SKIP_PLUGIN=true
		shift
		;;
	--binary)
		LOCAL_BINARY="$2"
		shift 2
		;;
	-h | --help)
		usage
		;;
	*)
		fatal "Unknown option: $1. Use --help for usage."
		;;
	esac
done

# Validate scope
case "$SCOPE" in
user | project | local) ;;
*) fatal "Invalid scope '$SCOPE'. Must be one of: user, project, local" ;;
esac

# ---------- Platform detection ----------

detect_platform() {
	local os arch target

	os="$(uname -s)"
	arch="$(uname -m)"

	case "$os" in
	Linux) os="linux" ;;
	Darwin) os="darwin" ;;
	*) fatal "Unsupported OS: $os. captain-hook supports Linux and macOS." ;;
	esac

	case "$arch" in
	x86_64 | amd64) arch="x86_64" ;;
	aarch64 | arm64) arch="aarch64" ;;
	*) fatal "Unsupported architecture: $arch. captain-hook supports x86_64 and aarch64." ;;
	esac

	case "${os}-${arch}" in
	linux-x86_64) target="x86_64-unknown-linux-gnu" ;;
	linux-aarch64) target="aarch64-unknown-linux-gnu" ;;
	darwin-x86_64) target="x86_64-apple-darwin" ;;
	darwin-aarch64) target="aarch64-apple-darwin" ;;
	*) fatal "Unsupported platform: ${os}-${arch}" ;;
	esac

	echo "$target"
}

# ---------- Version resolution ----------

resolve_version() {
	if [[ -n "$VERSION" ]]; then
		echo "$VERSION"
		return
	fi

	info "Fetching latest release version..."
	local latest
	latest="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" |
		grep '"tag_name"' |
		sed -E 's/.*"tag_name":\s*"v?([^"]+)".*/\1/')" ||
		fatal "Failed to fetch latest release. Check your network connection or specify --version."

	if [[ -z "$latest" ]]; then
		fatal "Could not determine latest version. Specify --version explicitly."
	fi

	echo "$latest"
}

# ---------- Binary download and verification ----------

download_binary() {
	local version="$1"
	local target="$2"
	local archive="captain-hook-v${version}-${target}.tar.gz"
	local url="https://github.com/${REPO}/releases/download/v${version}/${archive}"
	local sha_url="${url}.sha256"
	local tmpdir

	tmpdir="$(mktemp -d)"
	trap "rm -rf '$tmpdir'" EXIT

	info "Downloading ${archive}..."
	if ! curl -fsSL -o "${tmpdir}/${archive}" "$url"; then
		fatal "Download failed. Check that version v${version} exists and has a release for ${target}."
	fi

	# Verify checksum if available
	if curl -fsSL -o "${tmpdir}/${archive}.sha256" "$sha_url" 2>/dev/null; then
		info "Verifying SHA-256 checksum..."
		local expected actual
		expected="$(awk '{print $1}' "${tmpdir}/${archive}.sha256")"
		if command -v sha256sum &>/dev/null; then
			actual="$(sha256sum "${tmpdir}/${archive}" | awk '{print $1}')"
		elif command -v shasum &>/dev/null; then
			actual="$(shasum -a 256 "${tmpdir}/${archive}" | awk '{print $1}')"
		else
			warn "Neither sha256sum nor shasum found; skipping checksum verification."
			actual="$expected"
		fi

		if [[ "$expected" != "$actual" ]]; then
			fatal "Checksum mismatch!\n  Expected: ${expected}\n  Got:      ${actual}\nThe download may be corrupted. Try again or use --binary with a locally built binary."
		fi
		success "Checksum verified."
	else
		warn "No SHA-256 checksum file found for this release; skipping verification."
	fi

	# Extract
	info "Extracting binary..."
	tar -xzf "${tmpdir}/${archive}" -C "${tmpdir}"

	# Find the binary in the extracted contents
	local extracted_bin=""
	if [[ -f "${tmpdir}/captain-hook" ]]; then
		extracted_bin="${tmpdir}/captain-hook"
	elif [[ -f "${tmpdir}/captain-hook-v${version}-${target}/captain-hook" ]]; then
		extracted_bin="${tmpdir}/captain-hook-v${version}-${target}/captain-hook"
	else
		# Search for it
		extracted_bin="$(find "${tmpdir}" -name captain-hook -type f -perm +111 | head -1)" ||
			fatal "Could not find captain-hook binary in the archive."
	fi

	if [[ -z "$extracted_bin" ]]; then
		fatal "Could not find captain-hook binary in the archive."
	fi

	# Install
	mkdir -p "$INSTALL_DIR"
	cp "$extracted_bin" "${INSTALL_DIR}/captain-hook"
	chmod +x "${INSTALL_DIR}/captain-hook"

	# Clean up trap already set
	success "Installed captain-hook to ${INSTALL_DIR}/captain-hook"
}

install_local_binary() {
	local src="$1"
	if [[ ! -f "$src" ]]; then
		fatal "Binary not found: $src"
	fi
	if [[ ! -x "$src" ]]; then
		chmod +x "$src"
	fi
	mkdir -p "$INSTALL_DIR"
	cp "$src" "${INSTALL_DIR}/captain-hook"
	chmod +x "${INSTALL_DIR}/captain-hook"
	success "Installed local binary to ${INSTALL_DIR}/captain-hook"
}

# ---------- PATH setup ----------

ensure_path() {
	# Check if INSTALL_DIR is already in PATH
	if echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
		return
	fi

	local shell_name rc_file line_to_add
	shell_name="$(basename "${SHELL:-/bin/bash}")"

	case "$shell_name" in
	bash)
		rc_file="$HOME/.bashrc"
		line_to_add="export PATH=\"${INSTALL_DIR}:\$PATH\""
		;;
	zsh)
		rc_file="$HOME/.zshrc"
		line_to_add="export PATH=\"${INSTALL_DIR}:\$PATH\""
		;;
	fish)
		rc_file="$HOME/.config/fish/config.fish"
		line_to_add="fish_add_path ${INSTALL_DIR}"
		;;
	*)
		warn "Unknown shell '$shell_name'. Add ${INSTALL_DIR} to your PATH manually."
		return
		;;
	esac

	# Check if the line is already present
	if [[ -f "$rc_file" ]] && grep -qF "$INSTALL_DIR" "$rc_file"; then
		info "${INSTALL_DIR} is already referenced in ${rc_file}"
		return
	fi

	info "Adding ${INSTALL_DIR} to PATH in ${rc_file}..."
	echo "" >>"$rc_file"
	echo "# Added by captain-hook installer" >>"$rc_file"
	echo "$line_to_add" >>"$rc_file"
	success "Updated ${rc_file}"
	warn "Restart your shell or run: source ${rc_file}"

	# Also export for the current script session
	export PATH="${INSTALL_DIR}:$PATH"
}

# ---------- Plugin installation ----------

install_plugin() {
	local version="$1"

	step "Installing Claude Code plugin"

	# Create plugin directory structure
	mkdir -p "${PLUGIN_DIR}/.claude-plugin"
	mkdir -p "${PLUGIN_DIR}/hooks"
	mkdir -p "${PLUGIN_DIR}/skills/register"
	mkdir -p "${PLUGIN_DIR}/skills/disable"
	mkdir -p "${PLUGIN_DIR}/skills/enable"
	mkdir -p "${PLUGIN_DIR}/skills/switch"
	mkdir -p "${PLUGIN_DIR}/skills/status"
	mkdir -p "${PLUGIN_DIR}/agents"

	# -- .claude-plugin/plugin.json --
	cat >"${PLUGIN_DIR}/.claude-plugin/plugin.json" <<PJSON
{
  "name": "captain-hook",
  "version": "${version}",
  "description": "Intelligent permission gating for Claude Code"
}
PJSON
	info "Created plugin.json (version ${version})"

	# -- .claude-plugin/marketplace.json --
	cat >"${PLUGIN_DIR}/.claude-plugin/marketplace.json" <<'MKTJSON'
{
  "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
  "name": "captain-hook-local",
  "description": "Local captain-hook plugin marketplace",
  "owner": {
    "name": "Epiphytic",
    "email": "captain-hook@epiphytic.dev"
  },
  "plugins": [
    {
      "name": "captain-hook",
      "description": "Intelligent permission gating for Claude Code",
      "source": "./"
    }
  ]
}
MKTJSON
	info "Created marketplace.json"

	# -- hooks/hooks.json --
	cat >"${PLUGIN_DIR}/hooks/hooks.json" <<'HOOKS'
{
  "hooks": {
    "user_prompt_submit": [
      {
        "matcher": ".*",
        "command": "captain-hook session-check"
      }
    ],
    "PreToolUse": [
      {
        "matcher": ".*",
        "command": "captain-hook check"
      }
    ]
  }
}
HOOKS
	info "Created hooks.json"

	# -- skills/register/SKILL.md --
	cat >"${PLUGIN_DIR}/skills/register/SKILL.md" <<'SKILLEOF'
---
name: captain-hook register
description: Register the current session with a role for permission gating
---

# captain-hook register

Register this session with a role. The role determines what file paths and tool calls are permitted without human approval. Each session must be registered before captain-hook will allow tool calls.

## Instructions

1. Determine the session ID from the environment. Use the `SESSION_ID` environment variable if available, or derive it from the Claude Code session context.

2. Present the available roles to the user grouped by category. Use AskUserQuestion to let them choose:

   **Implementation roles** (write to specific code/config directories):
   - `coder` -- modify src/, lib/, project config (Cargo.toml, package.json, etc.)
   - `tester` -- modify tests/, test configs, coverage configs
   - `integrator` -- terraform, pulumi, CDK, ansible, helm files
   - `devops` -- CI/CD pipelines, Dockerfiles, tooling config files

   **Knowledge roles** (read codebase, write artifacts to docs/ subdirectories):
   - `researcher` -- write to docs/research/
   - `architect` -- write to docs/architecture/, docs/adr/
   - `planner` -- write to docs/plans/
   - `reviewer` -- write to docs/reviews/ (not security/)
   - `security-reviewer` -- write to docs/reviews/security/, run security scanners
   - `docs` -- write to docs/, *.md, *.aisp

   **Full-access roles** (unrestricted file access):
   - `maintainer` -- full repository access
   - `troubleshooter` -- full access for debugging

   **Other options:**
   - `disable` -- turn off captain-hook for this session

3. If the user chooses `disable`, run:
   captain-hook disable --session-id "$SESSION_ID"

4. Otherwise, register with the chosen role:
   captain-hook register --session-id "$SESSION_ID" --role <chosen-role>

5. Confirm the registration to the user, showing:
   - The registered role name
   - A summary of allowed and denied write paths for that role
   - A note that sensitive paths (.claude/, .env, etc.) always prompt regardless of role
SKILLEOF
	info "Created skills/register/SKILL.md"

	# -- skills/disable/SKILL.md --
	cat >"${PLUGIN_DIR}/skills/disable/SKILL.md" <<'SKILLEOF'
---
name: captain-hook disable
description: Disable captain-hook permission gating for this session
---

# captain-hook disable

Disable captain-hook for the current session. When disabled, all tool calls are permitted without permission gating.

## Instructions

1. Determine the session ID from the environment.

2. Run:
   captain-hook disable --session-id "$SESSION_ID"

3. Confirm to the user that captain-hook is disabled:
   - All tool calls will be permitted without gating
   - No path policies or role restrictions will be enforced
   - To re-enable: /captain-hook enable

4. If the command fails (e.g., session not found), report the error to the user.
SKILLEOF
	info "Created skills/disable/SKILL.md"

	# -- skills/enable/SKILL.md --
	cat >"${PLUGIN_DIR}/skills/enable/SKILL.md" <<'SKILLEOF'
---
name: captain-hook enable
description: Re-enable captain-hook permission gating for this session
---

# captain-hook enable

Re-enable captain-hook for a session that was previously disabled.

## Instructions

1. Determine the session ID from the environment.

2. Run:
   captain-hook enable --session-id "$SESSION_ID"

3. If the session was previously registered with a role, confirm re-enablement with:
   - The restored role name
   - The path policy summary for that role

4. If the session was never registered with a role (only disabled), prompt the user to choose a role using the same flow as /captain-hook register.

5. If the session is not currently disabled, inform the user that captain-hook is already active and show the current role.
SKILLEOF
	info "Created skills/enable/SKILL.md"

	# -- skills/switch/SKILL.md --
	cat >"${PLUGIN_DIR}/skills/switch/SKILL.md" <<'SKILLEOF'
---
name: captain-hook switch
description: Switch the current session to a different role
---

# captain-hook switch

Change the role for the current session. This clears cached decisions for the old role and applies the new role's path policies.

## Instructions

1. Determine the session ID from the environment.

2. If the user provided a role name as an argument (e.g., /captain-hook switch docs), use it directly.

3. If no role name was provided, present the available roles (same list as /captain-hook register) and ask the user to choose via AskUserQuestion.

4. Run:
   captain-hook register --session-id "$SESSION_ID" --role <new-role>

5. Confirm the role switch to the user, showing:
   - Previous role (if known)
   - New role
   - New path policy summary (allowed and denied write paths)
   - Note that cached decisions for the previous role have been cleared and will be re-evaluated under the new role
SKILLEOF
	info "Created skills/switch/SKILL.md"

	# -- skills/status/SKILL.md --
	cat >"${PLUGIN_DIR}/skills/status/SKILL.md" <<'SKILLEOF'
---
name: captain-hook status
description: Show the current session's captain-hook status and cache statistics
---

# captain-hook status

Display the current captain-hook status for this session, including role information, path policies, and cache statistics.

## Instructions

1. Run:
   captain-hook stats

2. Present the output to the user in a clear format, including:
   - Session ID: the current session identifier
   - Status: active (with role name), disabled, or unregistered
   - Role: the current role name and its description
   - Path policy: summary of allowed and denied write paths for the role
   - Cache statistics:
     - Total entries (allow / deny / ask breakdown)
     - Hit rate (percentage of tool calls resolved from cache)
     - Number of pending decisions in the queue
   - Sensitive paths: list of paths that always prompt regardless of role
SKILLEOF
	info "Created skills/status/SKILL.md"

	# -- agents/supervisor.md (abbreviated) --
	cat >"${PLUGIN_DIR}/agents/supervisor.md" <<'AGENTEOF'
---
name: captain-hook-supervisor
description: Permission evaluation supervisor agent for captain-hook
---

# captain-hook Supervisor Agent

You are the permission supervisor for a captain-hook agent team. Your role is to evaluate tool call permission requests from worker agents and make allow/deny/ask decisions based on the project's permission policy, role definitions, and task context.

NOTE: This is an abbreviated version installed by the captain-hook installer.
The full supervisor agent instructions are in the captain-hook repository at agents/supervisor.md.
See: https://github.com/Epiphytic/captain-hook/blob/main/agents/supervisor.md
AGENTEOF
	info "Created agents/supervisor.md (abbreviated)"

	success "Plugin files written to ${PLUGIN_DIR}/"

	# -- Register with Claude CLI if available --
	register_plugin_with_claude
}

register_plugin_with_claude() {
	if ! command -v claude &>/dev/null; then
		warn "Claude CLI not found in PATH."
		echo ""
		info "To use captain-hook as a Claude Code plugin, run Claude with:"
		echo "    claude --plugin-dir ${PLUGIN_DIR}"
		echo ""
		info "Or install the Claude CLI and re-run this script."
		return
	fi

	step "Registering plugin with Claude CLI"

	# The plugin directory doubles as a local marketplace (marketplace.json + plugin.json
	# both live in .claude-plugin/). Register it as a marketplace, then install the plugin.

	if claude plugin marketplace add "${PLUGIN_DIR}" 2>&1; then
		success "Added captain-hook-local marketplace to Claude."
	else
		warn "Could not register marketplace with Claude CLI."
		info "You can use captain-hook by running Claude with:"
		echo "    claude --plugin-dir ${PLUGIN_DIR}"
		return
	fi

	if claude plugin install "captain-hook@captain-hook-local" 2>&1; then
		success "Installed captain-hook plugin via Claude CLI."
	else
		warn "Could not install plugin via CLI. You can install it manually:"
		echo "    claude plugin install captain-hook@captain-hook-local"
	fi
}

# ---------- Verification ----------

verify_install() {
	step "Verifying installation"

	local bin="${INSTALL_DIR}/captain-hook"
	if [[ ! -x "$bin" ]]; then
		fatal "Binary not found or not executable at ${bin}"
	fi

	local ver_output
	if ver_output="$("$bin" --version 2>&1)"; then
		success "captain-hook is working: ${ver_output}"
	else
		warn "captain-hook binary exists but 'captain-hook --version' returned an error."
		warn "Output: ${ver_output}"
		info "The binary may still work. Try: ${bin} --help"
	fi
}

# ---------- Main ----------

main() {
	echo ""
	printf "${BOLD}captain-hook installer${NC}\n"
	echo "=============================="
	echo ""

	# Detect platform
	step "Detecting platform"
	local target
	target="$(detect_platform)"
	success "Platform: ${target}"

	# Install binary
	step "Installing binary"
	if [[ -n "$LOCAL_BINARY" ]]; then
		info "Using local binary: ${LOCAL_BINARY}"
		install_local_binary "$LOCAL_BINARY"
	else
		local version
		version="$(resolve_version)"
		info "Version: ${version}"
		download_binary "$version" "$target"
	fi

	# Ensure PATH
	step "Checking PATH"
	ensure_path

	# Install plugin
	if [[ "$SKIP_PLUGIN" == "true" ]]; then
		info "Skipping plugin setup (--skip-plugin)"
	else
		# Determine version for plugin.json
		local plugin_version="${VERSION:-}"
		if [[ -z "$plugin_version" ]]; then
			# Try to get version from the installed binary
			plugin_version="$("${INSTALL_DIR}/captain-hook" --version 2>/dev/null |
				grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)" || true
			if [[ -z "$plugin_version" ]]; then
				plugin_version="0.0.0"
			fi
		fi
		install_plugin "$plugin_version"
	fi

	# Verify
	verify_install

	# Summary
	echo ""
	echo "=============================="
	success "Installation complete!"
	echo ""
	info "Binary:  ${INSTALL_DIR}/captain-hook"
	if [[ "$SKIP_PLUGIN" != "true" ]]; then
		info "Plugin:  ${PLUGIN_DIR}/"
	fi
	echo ""
	info "Next steps:"
	echo "  1. Run 'captain-hook init' in your project to create .captain-hook/"
	echo "  2. Start Claude Code -- captain-hook will prompt for role registration"
	echo "  3. Use /captain-hook status to check your session"
	echo ""
}

main
