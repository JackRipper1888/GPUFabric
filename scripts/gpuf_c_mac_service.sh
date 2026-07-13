#!/usr/bin/env bash
set -euo pipefail

CONFIG_FILE="${GPUF_C_CONFIG:-$HOME/.gpuf-c.env}"
LOG_FILE="${GPUF_C_LOG:-$HOME/gpuf-c.log}"
PID_FILE="${GPUF_C_PID:-$HOME/.gpuf-c.pid}"

DEFAULT_CLIENT_ID="00112233445566778899aabbccddeeff"
DEFAULT_SERVER_ADDR="agent.gpunexus.com"
DEFAULT_CONTROL_PORT="17000"
DEFAULT_PROXY_PORT="17001"
DEFAULT_TLS_SERVER_NAME="agent.gpunexus.com"
DEFAULT_MODEL_PATH="/usr/local/share/gpuf-c/models/bge-m3-q8_0.gguf"
DEFAULT_CERT_CHAIN_PATH="/usr/local/bin/ca-cert.pem"
DEFAULT_BIN="/usr/local/bin/gpuf-c"
DEFAULT_LOCAL_PORT="11435"
DEFAULT_USE_SUDO="1"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

ensure_config() {
  if [[ -f "$CONFIG_FILE" ]]; then
    return
  fi

  {
    printf 'CLIENT_ID=%s\n' "$DEFAULT_CLIENT_ID"
    printf 'SERVER_ADDR=%s\n' "$DEFAULT_SERVER_ADDR"
    printf 'CONTROL_PORT=%s\n' "$DEFAULT_CONTROL_PORT"
    printf 'PROXY_PORT=%s\n' "$DEFAULT_PROXY_PORT"
    printf 'TLS_SERVER_NAME=%s\n' "$DEFAULT_TLS_SERVER_NAME"
    printf 'MODEL_PATH=%s\n' "$DEFAULT_MODEL_PATH"
    printf 'CERT_CHAIN_PATH=%s\n' "$DEFAULT_CERT_CHAIN_PATH"
    printf 'GPUF_C_BIN=%s\n' "$DEFAULT_BIN"
    printf 'LOCAL_PORT=%s\n' "$DEFAULT_LOCAL_PORT"
    printf 'USE_SUDO=%s\n' "$DEFAULT_USE_SUDO"
    printf 'RUST_LOG=%s\n' "gpuf_c=debug,common=info"
    printf 'N_GPU_LAYERS=%s\n' "99"
    printf 'N_CTX=%s\n' "2048"
    printf 'N_BATCH=%s\n' "512"
  } > "$CONFIG_FILE"
  chmod 600 "$CONFIG_FILE"
  echo "Created config: $CONFIG_FILE"
}

load_config() {
  ensure_config
  # shellcheck disable=SC1090
  source "$CONFIG_FILE"

  CLIENT_ID="${CLIENT_ID:-$DEFAULT_CLIENT_ID}"
  SERVER_ADDR="${SERVER_ADDR:-$DEFAULT_SERVER_ADDR}"
  CONTROL_PORT="${CONTROL_PORT:-$DEFAULT_CONTROL_PORT}"
  PROXY_PORT="${PROXY_PORT:-$DEFAULT_PROXY_PORT}"
  TLS_SERVER_NAME="${TLS_SERVER_NAME:-$DEFAULT_TLS_SERVER_NAME}"
  MODEL_PATH="${MODEL_PATH:-$DEFAULT_MODEL_PATH}"
  CERT_CHAIN_PATH="${CERT_CHAIN_PATH:-$DEFAULT_CERT_CHAIN_PATH}"
  GPUF_C_BIN="${GPUF_C_BIN:-$DEFAULT_BIN}"
  LOCAL_PORT="${LOCAL_PORT:-$DEFAULT_LOCAL_PORT}"
  USE_SUDO="${USE_SUDO:-$DEFAULT_USE_SUDO}"
  RUST_LOG="${RUST_LOG:-gpuf_c=debug,common=info}"
  N_GPU_LAYERS="${N_GPU_LAYERS:-99}"
  N_CTX="${N_CTX:-2048}"
  N_BATCH="${N_BATCH:-512}"
}

is_running() {
  [[ -f "$PID_FILE" ]] || return 1
  local pid
  pid="$(cat "$PID_FILE")"
  [[ -n "$pid" ]] || return 1
  kill -0 "$pid" >/dev/null 2>&1
}

stale_pid_cleanup() {
  if [[ -f "$PID_FILE" ]] && ! is_running; then
    rm -f "$PID_FILE"
  fi
}

validate_config() {
  [[ "$CLIENT_ID" =~ ^[0-9a-fA-F]{32}$ ]] || die "CLIENT_ID must be 32 hex characters: $CLIENT_ID"
  [[ -x "$GPUF_C_BIN" ]] || die "gpuf-c binary not executable: $GPUF_C_BIN"
  [[ -f "$MODEL_PATH" ]] || die "model file not found: $MODEL_PATH"
  [[ -f "$CERT_CHAIN_PATH" ]] || die "CA cert file not found: $CERT_CHAIN_PATH"
}

build_cmd() {
  CMD=(
    env "RUST_LOG=$RUST_LOG" "$GPUF_C_BIN"
    --client-id "$CLIENT_ID"
    --server-addr "$SERVER_ADDR"
    --control-port "$CONTROL_PORT"
    --proxy-port "$PROXY_PORT"
    --engine-type llama
    --llama-model-path "$MODEL_PATH"
    --n-gpu-layers "$N_GPU_LAYERS"
    --n-ctx "$N_CTX"
    --n-batch "$N_BATCH"
    --control-tls
    --control-tls-server-name "$TLS_SERVER_NAME"
    --cert-chain-path "$CERT_CHAIN_PATH"
    --local-port "$LOCAL_PORT"
  )

  if [[ "$USE_SUDO" == "1" ]]; then
    CMD=(sudo "${CMD[@]}")
  fi
}

sudo_preflight() {
  if [[ "$USE_SUDO" == "1" ]]; then
    sudo -v
  fi
}

kill_process() {
  local signal="$1"
  local pid="$2"
  if [[ "$USE_SUDO" == "1" ]]; then
    sudo kill "$signal" "$pid"
  else
    kill "$signal" "$pid"
  fi
}

print_cmd() {
  load_config
  build_cmd
  printf '%q ' "${CMD[@]}"
  printf '> %q 2>&1 &\n' "$LOG_FILE"
}

start() {
  load_config
  stale_pid_cleanup
  validate_config

  if is_running; then
    echo "gpuf-c is already running: pid=$(cat "$PID_FILE")"
    return
  fi

  mkdir -p "$(dirname "$LOG_FILE")" "$(dirname "$PID_FILE")"
  touch "$LOG_FILE"
  build_cmd
  sudo_preflight

  nohup "${CMD[@]}" > "$LOG_FILE" 2>&1 &

  echo $! > "$PID_FILE"
  sleep 1
  if ! is_running; then
    rm -f "$PID_FILE"
    echo "gpuf-c failed to stay running. Last 80 log lines:" >&2
    tail -n 80 "$LOG_FILE" >&2 || true
    exit 1
  fi

  echo "Started gpuf-c: pid=$(cat "$PID_FILE")"
  echo "Log: $LOG_FILE"
}

stop() {
  load_config
  if is_running; then
    local pid
    pid="$(cat "$PID_FILE")"
    sudo_preflight
    kill_process -TERM "$pid"
    for _ in {1..20}; do
      if ! kill -0 "$pid" >/dev/null 2>&1; then
        rm -f "$PID_FILE"
        echo "Stopped gpuf-c: pid=$pid"
        return
      fi
      sleep 0.5
    done
    echo "gpuf-c did not stop after SIGTERM, sending SIGKILL: pid=$pid"
    kill_process -KILL "$pid" >/dev/null 2>&1 || true
    rm -f "$PID_FILE"
    echo "Stopped gpuf-c: pid=$pid"
    return
  fi

  echo "gpuf-c is not running by pid file"
  stale_pid_cleanup
}

status() {
  load_config
  stale_pid_cleanup
  if is_running; then
    echo "gpuf-c running"
    echo "pid: $(cat "$PID_FILE")"
    ps -p "$(cat "$PID_FILE")" -o pid,ppid,etime,command || true
  else
    echo "gpuf-c not running"
  fi
  echo "client_id: $CLIENT_ID"
  echo "server: $SERVER_ADDR:$CONTROL_PORT/$PROXY_PORT"
  echo "tls_server_name: $TLS_SERVER_NAME"
  echo "model: $MODEL_PATH"
  echo "binary: $GPUF_C_BIN"
  echo "log: $LOG_FILE"
  echo "config: $CONFIG_FILE"
}

logs() {
  touch "$LOG_FILE"
  tail -f "$LOG_FILE"
}

tail_log() {
  local lines="${1:-120}"
  touch "$LOG_FILE"
  tail -n "$lines" "$LOG_FILE"
}

errors() {
  touch "$LOG_FILE"
  grep -Ei 'error|warn|panic|abort|broken pipe|connection reset|failed' "$LOG_FILE" | tail -n "${1:-120}" || true
}

doctor() {
  load_config
  echo "Config file: $CONFIG_FILE"
  validate_config
  echo "Config validation: OK"
  status
  echo
  echo "Recent warnings/errors:"
  errors 40
}

rotate_log() {
  local ts
  ts="$(date +%Y%m%d_%H%M%S)"
  if [[ -f "$LOG_FILE" ]]; then
    mv "$LOG_FILE" "$LOG_FILE.$ts"
    touch "$LOG_FILE"
    echo "Rotated log to: $LOG_FILE.$ts"
  else
    touch "$LOG_FILE"
    echo "Created log: $LOG_FILE"
  fi
}

set_client_id() {
  local client_id="${1:-}"
  if [[ -z "$client_id" ]]; then
    echo "Usage: $0 client-id <client_id>" >&2
    exit 2
  fi
  [[ "$client_id" =~ ^[0-9a-fA-F]{32}$ ]] || die "client_id must be 32 hex characters"

  ensure_config
  if grep -q '^CLIENT_ID=' "$CONFIG_FILE"; then
    sed -i.bak "s/^CLIENT_ID=.*/CLIENT_ID=$client_id/" "$CONFIG_FILE"
  else
    printf '\nCLIENT_ID=%s\n' "$client_id" >> "$CONFIG_FILE"
  fi
  echo "Updated CLIENT_ID in $CONFIG_FILE"
}

set_value() {
  local key="${1:-}"
  local value="${2:-}"
  if [[ -z "$key" || -z "$value" ]]; then
    echo "Usage: $0 set <KEY> <VALUE>" >&2
    exit 2
  fi
  [[ "$key" =~ ^[A-Z0-9_]+$ ]] || die "invalid key: $key"

  ensure_config
  if grep -q "^$key=" "$CONFIG_FILE"; then
    sed -i.bak "s|^$key=.*|$key=$value|" "$CONFIG_FILE"
  else
    printf '\n%s=%s\n' "$key" "$value" >> "$CONFIG_FILE"
  fi
  echo "Updated $key in $CONFIG_FILE"
}

usage() {
  printf 'Usage: %s <command>\n\n' "$0"
  printf 'Commands:\n'
  printf '  init                  Create default config if missing\n'
  printf '  start                 Start gpuf-c in background\n'
  printf '  stop                  Stop gpuf-c by pid file\n'
  printf '  restart               Stop then start gpuf-c\n'
  printf '  status                Show process and config status\n'
  printf '  logs                  Follow log file\n'
  printf '  tail [lines]          Print recent log lines, default 120\n'
  printf '  errors [lines]        Print recent warning/error lines, default 120\n'
  printf '  doctor                Validate config and show recent warnings/errors\n'
  printf '  rotate-log            Move current log to a timestamped backup\n'
  printf '  cmd                   Print the exact start command\n'
  printf '  client-id <client_id> Update CLIENT_ID in %s\n\n' "$CONFIG_FILE"
  printf '  set <KEY> <VALUE>     Update one config item, e.g. MODEL_PATH\n\n'
  printf 'Config:\n'
  printf '  %s\n\n' "$CONFIG_FILE"
  printf 'Examples:\n'
  printf '  %s init\n' "$0"
  printf '  %s set MODEL_PATH /usr/local/share/gpuf-c/models/bge-m3-q8_0.gguf\n' "$0"
  printf '  %s start\n' "$0"
  printf '  %s logs\n' "$0"
  printf '  %s client-id 00112233445566778899aabbccddeeff\n' "$0"
  printf '  %s restart\n' "$0"
}

cmd="${1:-}"
case "$cmd" in
  init)
    ensure_config
    echo "Config: $CONFIG_FILE"
    ;;
  start)
    start
    ;;
  stop)
    stop
    ;;
  restart)
    stop || true
    start
    ;;
  status)
    status
    ;;
  logs)
    logs
    ;;
  tail)
    shift
    tail_log "${1:-120}"
    ;;
  errors)
    shift
    errors "${1:-120}"
    ;;
  doctor)
    doctor
    ;;
  rotate-log)
    rotate_log
    ;;
  cmd)
    print_cmd
    ;;
  client-id)
    shift
    set_client_id "${1:-}"
    ;;
  set)
    shift
    set_value "${1:-}" "${2:-}"
    ;;
  *)
    usage
    exit 2
    ;;
esac
