#!/usr/bin/env bash
set -euo pipefail

umask 077

usage() {
    cat >&2 <<'USAGE'
Usage:
  manage_benchmark_keyring.sh issue KEY_ID PRIVATE_KEY KEYRING_JSON [VALID_DAYS] [PURPOSE]
  manage_benchmark_keyring.sh transition KEY_ID retired|revoked KEYRING_JSON

issue creates an Ed25519 private key with mode 0600 and atomically adds an
active managed public-key entry. transition is monotonic: active -> retired or
revoked, and retired -> revoked. PURPOSE is test_only by default and may be
performance_claim only for an approved real benchmark runner. A revoked key
cannot be reactivated.
USAGE
    exit 2
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || {
        printf 'required command not found: %s\n' "$1" >&2
        exit 2
    }
}

validate_key_id() {
    local key_id="$1"
    if [[ ! "$key_id" =~ ^[A-Za-z0-9._:-]{1,64}$ ]]; then
        printf 'KEY_ID must contain 1-64 ASCII letters, digits, dot, underscore, colon, or dash\n' >&2
        exit 2
    fi
}

prepare_keyring_base() {
    local keyring_file="$1"
    local output_file="$2"
    if [[ -L "$keyring_file" ]]; then
        printf 'refusing symbolic-link keyring: %s\n' "$keyring_file" >&2
        exit 2
    fi
    if [[ -e "$keyring_file" ]]; then
        jq -e 'type == "object" and length <= 16' "$keyring_file" >/dev/null
        cp "$keyring_file" "$output_file"
    else
        printf '{}\n' >"$output_file"
    fi
}

atomic_replace_keyring() {
    local source_file="$1"
    local keyring_file="$2"
    chmod 600 "$source_file"
    mv "$source_file" "$keyring_file"
}

issue_key() {
    [[ $# -ge 3 && $# -le 5 ]] || usage
    local key_id="$1"
    local private_key="$2"
    local keyring_file="$3"
    local valid_days="${4:-365}"
    local purpose="${5:-test_only}"
    validate_key_id "$key_id"
    if [[ ! "$valid_days" =~ ^[1-9][0-9]*$ ]] || ((valid_days > 730)); then
        printf 'VALID_DAYS must be between 1 and 730\n' >&2
        exit 2
    fi
    if [[ "$purpose" != test_only && "$purpose" != performance_claim ]]; then
        printf 'PURPOSE must be test_only or performance_claim\n' >&2
        exit 2
    fi
    if [[ -e "$private_key" || -L "$private_key" ]]; then
        printf 'refusing to overwrite private key: %s\n' "$private_key" >&2
        exit 2
    fi
    [[ -d "$(dirname "$private_key")" ]] || {
        printf 'private-key directory does not exist\n' >&2
        exit 2
    }
    [[ -d "$(dirname "$keyring_file")" ]] || {
        printf 'keyring directory does not exist\n' >&2
        exit 2
    }

    local work_dir
    work_dir="$(mktemp -d)"
    local base_file="$work_dir/keyring-base.json"
    local next_file
    next_file="$(mktemp "$(dirname "$keyring_file")/.benchmark-keyring.XXXXXX")"
    local completed=false
    cleanup() {
        rm -rf "$work_dir"
        if [[ "$completed" != true ]]; then
            rm -f "$private_key" "$next_file"
        fi
    }
    trap cleanup EXIT

    prepare_keyring_base "$keyring_file" "$base_file"
    if jq -e --arg key_id "$key_id" 'has($key_id)' "$base_file" >/dev/null; then
        printf 'KEY_ID already exists in keyring: %s\n' "$key_id" >&2
        exit 2
    fi
    if (( $(jq 'length' "$base_file") >= 16 )); then
        printf 'keyring already contains the maximum 16 keys\n' >&2
        exit 2
    fi

    openssl genpkey -algorithm ED25519 -out "$private_key"
    chmod 600 "$private_key"
    local public_der="$work_dir/public.der"
    openssl pkey -in "$private_key" -pubout -outform DER -out "$public_der"
    if [[ "$(wc -c <"$public_der")" -lt 32 ]]; then
        printf 'unexpected Ed25519 public-key encoding\n' >&2
        exit 1
    fi
    local public_key_base64
    public_key_base64="$(tail -c 32 "$public_der" | openssl base64 -A)"
    local not_before
    local not_after
    not_before="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
    not_after="$(date -u -d "+${valid_days} days" +'%Y-%m-%dT%H:%M:%SZ')"

    jq \
        --arg key_id "$key_id" \
        --arg public_key "$public_key_base64" \
        --arg not_before "$not_before" \
        --arg not_after "$not_after" \
        --arg purpose "$purpose" \
        '. + {($key_id): {
            publicKeyBase64: $public_key,
            status: "active",
            purpose: $purpose,
            notBefore: $not_before,
            notAfter: $not_after
        }}' \
        "$base_file" >"$next_file"
    atomic_replace_keyring "$next_file" "$keyring_file"
    completed=true
    rm -rf "$work_dir"
    trap - EXIT

    local fingerprint
    fingerprint="$(printf '%s' "$public_key_base64" | sha256sum | awk '{print $1}')"
    printf 'issued keyId=%s purpose=%s privateKey=%s keyring=%s publicKeyBase64Sha256=%s notAfter=%s\n' \
        "$key_id" "$purpose" "$private_key" "$keyring_file" "$fingerprint" "$not_after"
}

transition_key() {
    [[ $# -eq 3 ]] || usage
    local key_id="$1"
    local target_status="$2"
    local keyring_file="$3"
    validate_key_id "$key_id"
    if [[ "$target_status" != retired && "$target_status" != revoked ]]; then
        printf 'target status must be retired or revoked\n' >&2
        exit 2
    fi
    if [[ ! -f "$keyring_file" || -L "$keyring_file" ]]; then
        printf 'managed keyring is not a regular file: %s\n' "$keyring_file" >&2
        exit 2
    fi
    jq -e --arg key_id "$key_id" \
        'has($key_id) and (.[$key_id] | type == "object")' \
        "$keyring_file" >/dev/null
    local current_status
    current_status="$(jq -r --arg key_id "$key_id" '.[$key_id].status' "$keyring_file")"
    case "$current_status:$target_status" in
        active:retired|active:revoked|retired:revoked|retired:retired|revoked:revoked) ;;
        *)
            printf 'invalid or non-monotonic key transition: %s -> %s\n' \
                "$current_status" "$target_status" >&2
            exit 2
            ;;
    esac

    local next_file
    next_file="$(mktemp "$(dirname "$keyring_file")/.benchmark-keyring.XXXXXX")"
    trap 'rm -f "$next_file"' EXIT
    jq --arg key_id "$key_id" --arg status "$target_status" \
        '.[$key_id].status = $status' "$keyring_file" >"$next_file"
    atomic_replace_keyring "$next_file" "$keyring_file"
    trap - EXIT
    printf 'transitioned keyId=%s status=%s keyring=%s\n' \
        "$key_id" "$target_status" "$keyring_file"
}

for command in openssl jq date tail sha256sum awk wc mktemp cp mv chmod dirname rm stat; do
    require_command "$command"
done

[[ $# -ge 1 ]] || usage
command_name="$1"
shift
case "$command_name" in
    issue) issue_key "$@" ;;
    transition) transition_key "$@" ;;
    *) usage ;;
esac
