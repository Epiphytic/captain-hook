#!/usr/bin/env bash
#
# Self-Hosted GitHub Actions Runner Setup Script
# ==============================================
# This script configures and manages a self-hosted runner for the captain-hook repository.
# It is idempotent - running it multiple times will only perform necessary actions.
#
# Usage:
#   ./scripts/setup-self-hosted-runner.sh [OPTIONS] [COMMAND]
#
# Commands:
#   setup              Install and configure the runner (default)
#   start              Start the runner if not running
#   stop               Stop the runner
#   status             Check runner status
#   restart            Stop and start the runner
#   uninstall          Remove the runner completely
#
# Options:
#   --name NAME        Runner name (default: captain-hook-runner-<hostname>)
#   --labels LABELS    Comma-separated labels (default: auto-detected)
#   --work-dir DIR     Work directory (default: _work)
#   --token TOKEN      Runner registration token (auto-fetched via gh if not provided)
#   --repo OWNER/REPO  Repository (default: Epiphytic/captain-hook)
#   --help             Show this help message
#
# Prerequisites:
#   - gh (GitHub CLI) - for automatic token generation
#   - curl
#   - tar (Linux/macOS) or unzip (Windows)
#
# The script will automatically:
#   1. Detect your platform (OS and architecture)
#   2. Generate a runner registration token via gh CLI
#   3. Download and configure the GitHub Actions runner (if not already done)
#   4. Start the runner (if not already running)
#

set -euo pipefail

# ============================================================================
# Configuration
# ============================================================================

REPO="${GITHUB_RUNNER_REPO:-Epiphytic/captain-hook}"
REPO_URL="https://github.com/${REPO}"
RUNNER_VERSION="${GITHUB_RUNNER_VERSION:-2.321.0}"
RUNNER_NAME="${GITHUB_RUNNER_NAME:-captain-hook-runner-$(hostname -s)}"
RUNNER_LABELS="${GITHUB_RUNNER_LABELS:-}"
RUNNER_WORK_DIR="${GITHUB_RUNNER_WORK_DIR:-_work}"
RUNNER_TOKEN="${GITHUB_RUNNER_TOKEN:-}"

# Derived paths
RUNNER_BASE_DIR="${HOME}/actions-runner-${RUNNER_NAME}"
PID_FILE="${RUNNER_BASE_DIR}/.runner.pid"
CONFIG_FILE="${RUNNER_BASE_DIR}/.runner"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# ============================================================================
# Helper Functions
# ============================================================================

log_info() {
	echo -e "${BLUE}[INFO]${NC} $1" >&2
}

log_success() {
	echo -e "${GREEN}[SUCCESS]${NC} $1" >&2
}

log_warn() {
	echo -e "${YELLOW}[WARN]${NC} $1" >&2
}

log_error() {
	echo -e "${RED}[ERROR]${NC} $1" >&2
}

show_help() {
	head -40 "$0" | tail -35 | sed 's/^#//' | sed 's/^ //'
	exit 0
}

check_gh_cli() {
	if ! command -v gh &>/dev/null; then
		log_error "GitHub CLI (gh) is not installed."
		echo ""
		echo "Install it from: https://cli.github.com/"
		echo ""
		echo "  macOS:   brew install gh"
		echo "  Linux:   See https://github.com/cli/cli/blob/trunk/docs/install_linux.md"
		echo "  Windows: winget install GitHub.cli"
		echo ""
		exit 1
	fi

	if ! gh auth status &>/dev/null; then
		log_error "GitHub CLI is not authenticated."
		echo ""
		echo "Run: gh auth login"
		echo ""
		exit 1
	fi
}

detect_os() {
	case "$(uname -s)" in
	Linux*) echo "linux" ;;
	Darwin*) echo "osx" ;;
	MINGW* | MSYS* | CYGWIN*) echo "win" ;;
	*) echo "unknown" ;;
	esac
}

detect_arch() {
	case "$(uname -m)" in
	x86_64 | amd64) echo "x64" ;;
	aarch64 | arm64) echo "arm64" ;;
	armv7l) echo "arm" ;;
	*) echo "x64" ;;
	esac
}

get_runner_url() {
	local os="$1"
	local arch="$2"
	local version="$3"

	local ext="tar.gz"
	if [[ "$os" == "win" ]]; then
		ext="zip"
	fi

	echo "https://github.com/actions/runner/releases/download/v${version}/actions-runner-${os}-${arch}-${version}.${ext}"
}

get_runner_token() {
	log_info "Fetching runner registration token via gh CLI..."
	local token
	token=$(gh api "repos/${REPO}/actions/runners/registration-token" -X POST --jq '.token' 2>/dev/null | tr -d '\n\r')

	if [[ -z "$token" ]]; then
		log_error "Failed to get runner registration token."
		echo ""
		echo "Make sure you have admin access to the repository: ${REPO}"
		echo "Or provide a token manually with --token"
		echo ""
		exit 1
	fi

	printf '%s' "$token"
}

generate_labels() {
	local os="$1"
	local arch="$2"

	local labels="self-hosted,captain-hook"

	case "$os" in
	linux) labels="${labels},linux" ;;
	osx) labels="${labels},macos" ;;
	win) labels="${labels},windows" ;;
	esac

	labels="${labels},${arch}"
	echo "$labels"
}

# ============================================================================
# Process Management Functions
# ============================================================================

# Check if a process is running by PID
is_process_running() {
	local pid="$1"
	if [[ -z "$pid" ]]; then
		return 1
	fi

	# Check if process exists and is our runner
	if kill -0 "$pid" 2>/dev/null; then
		# Verify it's actually the runner process, not a recycled PID
		if [[ "$(detect_os)" == "osx" ]]; then
			ps -p "$pid" -o command= 2>/dev/null | grep -q "Runner.Listener" && return 0
		else
			ps -p "$pid" -o cmd= 2>/dev/null | grep -q "Runner.Listener" && return 0
		fi
	fi
	return 1
}

# Get the PID of a running runner
get_runner_pid() {
	# First check PID file
	if [[ -f "$PID_FILE" ]]; then
		local pid
		pid=$(cat "$PID_FILE" 2>/dev/null | tr -d '\n\r')
		if is_process_running "$pid"; then
			echo "$pid"
			return 0
		fi
		# Stale PID file - remove it
		rm -f "$PID_FILE"
	fi

	# Fallback: search for running runner process
	# Pattern: runner name appears in path BEFORE Runner.Listener
	local pid
	pid=$(pgrep -f "${RUNNER_NAME}.*Runner.Listener" 2>/dev/null | head -1)

	if [[ -n "$pid" ]] && is_process_running "$pid"; then
		# Update PID file
		echo "$pid" >"$PID_FILE"
		echo "$pid"
		return 0
	fi

	return 1
}

# Check if runner is configured
is_runner_configured() {
	[[ -f "$CONFIG_FILE" ]]
}

# Check if runner binaries are installed
is_runner_installed() {
	[[ -f "${RUNNER_BASE_DIR}/run.sh" ]] && [[ -f "${RUNNER_BASE_DIR}/config.sh" ]]
}

# ============================================================================
# Runner Commands
# ============================================================================

cmd_status() {
	echo "Runner: ${RUNNER_NAME}"
	echo "Directory: ${RUNNER_BASE_DIR}"
	echo ""

	if ! is_runner_installed; then
		echo "Status: NOT INSTALLED"
		return 1
	fi

	if ! is_runner_configured; then
		echo "Status: INSTALLED but NOT CONFIGURED"
		return 1
	fi

	local pid
	if pid=$(get_runner_pid); then
		echo -e "Status: ${GREEN}RUNNING${NC} (PID: $pid)"
		return 0
	else
		echo -e "Status: ${YELLOW}STOPPED${NC}"
		return 1
	fi
}

cmd_start() {
	if ! is_runner_installed; then
		log_error "Runner is not installed. Run setup first."
		return 1
	fi

	if ! is_runner_configured; then
		log_error "Runner is not configured. Run setup first."
		return 1
	fi

	# Check if already running
	local existing_pid
	if existing_pid=$(get_runner_pid); then
		log_warn "Runner is already running (PID: $existing_pid)"
		return 0
	fi

	# Clean up any stale PID file
	rm -f "$PID_FILE"

	log_info "Starting runner..."
	cd "$RUNNER_BASE_DIR"

	# Start runner in background
	nohup ./run.sh >"${RUNNER_BASE_DIR}/runner.log" 2>&1 &

	# Wait for Runner.Listener to actually start (run.sh spawns it as child)
	local count=0
	local pid=""
	while [[ $count -lt 10 ]]; do
		sleep 1
		# Find the actual Runner.Listener process (runner name is in path before Runner.Listener)
		pid=$(pgrep -f "${RUNNER_NAME}.*Runner.Listener" 2>/dev/null | head -1)
		if [[ -n "$pid" ]]; then
			break
		fi
		((count++))
	done

	# Verify it started
	if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
		echo "$pid" >"$PID_FILE"
		log_success "Runner started (PID: $pid)"
		echo "Log file: ${RUNNER_BASE_DIR}/runner.log" >&2
		return 0
	else
		log_error "Runner failed to start. Check ${RUNNER_BASE_DIR}/runner.log"
		return 1
	fi
}

cmd_stop() {
	local pid
	if ! pid=$(get_runner_pid); then
		log_warn "Runner is not running"
		rm -f "$PID_FILE"
		return 0
	fi

	log_info "Stopping runner (PID: $pid)..."

	# Send SIGTERM first for graceful shutdown
	kill -TERM "$pid" 2>/dev/null || true

	# Wait up to 10 seconds for graceful shutdown
	local count=0
	while is_process_running "$pid" && [[ $count -lt 10 ]]; do
		sleep 1
		((count++))
	done

	# Force kill if still running
	if is_process_running "$pid"; then
		log_warn "Runner didn't stop gracefully, forcing..."
		kill -9 "$pid" 2>/dev/null || true
		sleep 1
	fi

	rm -f "$PID_FILE"

	if is_process_running "$pid"; then
		log_error "Failed to stop runner"
		return 1
	else
		log_success "Runner stopped"
		return 0
	fi
}

cmd_restart() {
	cmd_stop
	sleep 1
	cmd_start
}

cmd_uninstall() {
	log_info "Uninstalling runner..."

	# Stop if running
	cmd_stop || true

	if [[ -d "$RUNNER_BASE_DIR" ]]; then
		# Try to unconfigure first
		if is_runner_configured; then
			log_info "Removing runner configuration..."
			check_gh_cli
			local token
			token=$(gh api "repos/${REPO}/actions/runners/remove-token" -X POST --jq '.token' 2>/dev/null | tr -d '\n\r')
			if [[ -n "$token" ]]; then
				cd "$RUNNER_BASE_DIR"
				./config.sh remove --token "$token" 2>/dev/null || true
			fi
		fi

		log_info "Removing runner directory..."
		rm -rf "$RUNNER_BASE_DIR"
	fi

	log_success "Runner uninstalled"
}

cmd_setup() {
	local os arch
	os=$(detect_os)
	arch=$(detect_arch)

	if [[ "$os" == "unknown" ]]; then
		log_error "Unsupported operating system"
		exit 1
	fi

	log_info "GitHub Actions Self-Hosted Runner Setup"
	log_info "========================================"
	log_info "Detected platform: ${os}-${arch}"
	log_info "Repository: ${REPO}"
	log_info "Runner name: ${RUNNER_NAME}"

	# Auto-generate labels if not provided
	if [[ -z "$RUNNER_LABELS" ]]; then
		RUNNER_LABELS=$(generate_labels "$os" "$arch")
	fi
	log_info "Runner labels: ${RUNNER_LABELS}"

	# Check if already fully set up and running
	if is_runner_installed && is_runner_configured; then
		local pid
		if pid=$(get_runner_pid); then
			log_success "Runner is already configured and running (PID: $pid)"
			return 0
		else
			log_info "Runner is configured but not running. Starting..."
			cmd_start
			return $?
		fi
	fi

	# Install if needed
	if ! is_runner_installed; then
		install_runner "$os" "$arch"
	else
		log_info "Runner binaries already installed"
	fi

	# Configure if needed
	if ! is_runner_configured; then
		configure_runner
	else
		log_info "Runner already configured"
	fi

	# Start if not running
	local pid
	if ! pid=$(get_runner_pid); then
		cmd_start
	else
		log_success "Runner is already running (PID: $pid)"
	fi
}

install_runner() {
	local os="$1"
	local arch="$2"

	log_info "Installing runner..."

	mkdir -p "$RUNNER_BASE_DIR"
	cd "$RUNNER_BASE_DIR"

	local runner_url
	runner_url=$(get_runner_url "$os" "$arch" "$RUNNER_VERSION")
	local runner_archive="actions-runner.tar.gz"

	if [[ "$os" == "win" ]]; then
		runner_archive="actions-runner.zip"
	fi

	log_info "Downloading runner from: ${runner_url}"
	curl -fsSL -o "$runner_archive" "$runner_url"

	log_info "Extracting runner..."
	if [[ "$os" == "win" ]]; then
		unzip -q "$runner_archive"
	else
		tar xzf "$runner_archive"
	fi
	rm "$runner_archive"

	log_success "Runner binaries installed"
}

configure_runner() {
	log_info "Configuring runner..."

	# Get token if not provided
	if [[ -z "$RUNNER_TOKEN" ]]; then
		check_gh_cli
		RUNNER_TOKEN=$(get_runner_token)
		log_success "Runner token obtained"
	fi

	cd "$RUNNER_BASE_DIR"

	./config.sh \
		--url "$REPO_URL" \
		--token "$RUNNER_TOKEN" \
		--name "$RUNNER_NAME" \
		--labels "$RUNNER_LABELS" \
		--work "$RUNNER_WORK_DIR" \
		--unattended \
		--replace

	log_success "Runner configured"
}

# ============================================================================
# Parse Arguments
# ============================================================================

COMMAND="setup"

while [[ $# -gt 0 ]]; do
	case $1 in
	--name)
		RUNNER_NAME="$2"
		RUNNER_BASE_DIR="${HOME}/actions-runner-${RUNNER_NAME}"
		PID_FILE="${RUNNER_BASE_DIR}/.runner.pid"
		CONFIG_FILE="${RUNNER_BASE_DIR}/.runner"
		shift 2
		;;
	--labels)
		RUNNER_LABELS="$2"
		shift 2
		;;
	--work-dir)
		RUNNER_WORK_DIR="$2"
		shift 2
		;;
	--token)
		RUNNER_TOKEN="$2"
		shift 2
		;;
	--repo)
		REPO="$2"
		REPO_URL="https://github.com/${REPO}"
		shift 2
		;;
	--help | -h)
		show_help
		;;
	setup | start | stop | status | restart | uninstall)
		COMMAND="$1"
		shift
		;;
	*)
		log_error "Unknown option: $1"
		exit 1
		;;
	esac
done

# ============================================================================
# Main
# ============================================================================

case "$COMMAND" in
setup)
	cmd_setup
	;;
start)
	cmd_start
	;;
stop)
	cmd_stop
	;;
status)
	cmd_status
	;;
restart)
	cmd_restart
	;;
uninstall)
	cmd_uninstall
	;;
*)
	log_error "Unknown command: $COMMAND"
	exit 1
	;;
esac
