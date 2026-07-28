#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool_dir="$(cd "$script_dir/.." && pwd)"
repo_dir="$(cd "$tool_dir/../.." && pwd)"
asset_dir="${ASSESSMENT_SERVICE_DIR:-$(cd "$repo_dir/../asset-assessment-service" && pwd)}"
env_file="${GPUF_TEST_ENV_FILE:-$repo_dir/docker/.env.gpuf-s-test}"
compose_file="$tool_dir/deploy/compose.yaml"
gpuf_compose_file="$repo_dir/docker/gpuf_s_test_compose.yaml"
runtime_dir="${ASSESSMENT_E2E_RUNTIME_DIR:-/tmp/gpuf-assessment-runtime}"
image="${ASSESSMENT_E2E_IMAGE:-gpuf/asset-assessment-service:e2e-local}"

usage() {
  echo "usage: $0 {run|up|down|reset}"
  echo "  run    build, start and execute the complete local E2E flow (default)"
  echo "  up     build and start the local services without running the flow"
  echo "  down   stop only the asset-assessment E2E services"
  echo "  reset  stop the asset-assessment E2E services and remove test volumes"
}

require_file() {
  if [[ ! -f "$1" ]]; then
    echo "required file not found: $1" >&2
    exit 1
  fi
}

load_environment() {
  require_file "$env_file"
  set -a
  # shellcheck disable=SC1090
  source "$env_file"
  set +a
  local banking_token="${GPUF_TEST_BANKING_API_TOKEN:-}"
  if [[ ${#banking_token} -lt 32 ]]; then
    echo "GPUF_TEST_BANKING_API_TOKEN must contain at least 32 characters" >&2
    exit 1
  fi
  export ASSESSMENT_SERVICE_DIR="$asset_dir"
  export ASSESSMENT_E2E_IMAGE="$image"
}

compose() {
  docker compose --env-file "$env_file" -f "$compose_file" "$@"
}

ensure_gpufabric() {
  docker compose --env-file "$env_file" -f "$gpuf_compose_file" up -d --wait
  curl --fail --silent --show-error "http://127.0.0.1:${GPUF_TEST_API_PORT:-18181}/api/models/get" >/dev/null
}

build_assessment() {
  require_file "$asset_dir/go.mod"
  mkdir -p "$runtime_dir"
  (
    cd "$asset_dir"
    CGO_ENABLED=0 go build -buildvcs=false -trimpath -ldflags="-s -w" \
      -o "$runtime_dir/asset-assessment-service" ./cmd/server
  )
  chmod 0755 "$runtime_dir/asset-assessment-service"
  sha256sum "$runtime_dir/asset-assessment-service"
  docker build --network host -f "$tool_dir/Dockerfile.assessment-runtime" -t "$image" "$runtime_dir"
}

start_services() {
  ensure_gpufabric
  build_assessment
  compose up -d --build --wait
}

run_flow() {
  export SSL_CERT_FILE=/tmp/gpuf-asset-assessment-e2e-minio-tls/ca.crt
  (
    cd "$tool_dir"
    go run ./cmd/runner
  )
}

command="${1:-run}"
case "$command" in
  run)
    load_environment
    start_services
    run_flow
    ;;
  up)
    load_environment
    start_services
    ;;
  down)
    load_environment
    compose down --remove-orphans
    ;;
  reset)
    load_environment
    compose down --volumes --remove-orphans
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
