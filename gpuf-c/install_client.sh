#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# init log file
LOG_FILE="/tmp/gpuf_c_llamacpp_install_$(date +%Y%m%d_%H%M%S).log"
echo "安装开始于 $(date)" > "$LOG_FILE"

# log function
log() {
    echo -e "$1"
    echo -e "[$(date +'%Y-%m-%d %H:%M:%S')] $1" >> "$LOG_FILE"
}

# check command exists
check_command() {
    if ! command -v "$1" &> /dev/null; then
        log "${RED}错误：需要 $1 但未安装${NC}"
        exit 1
    fi
}

verify_macos_binary_format() {
    local file_path="$1"

    if [ "$OS" != "darwin" ]; then
        return 0
    fi

    if ! command -v file &> /dev/null; then
        log "${YELLOW}警告：未找到 'file' 命令；跳过 macOS 二进制格式检查${NC}"
        return 0
    fi

    local out
    out=$(file "$file_path" 2>/dev/null || true)
    if [[ "$out" != *"Mach-O"* ]]; then
        log "${RED}macOS 的 gpuf-c 二进制文件无效（期望 Mach-O，但得到其他格式）${NC}"
        log "${YELLOW}$out${NC}"
        log "${YELLOW}提示：您的下载包可能错误（例如，Linux 压缩包上传到 mac 密钥）${NC}"
        return 1
    fi

    local mach
    mach=$(uname -m | tr '[:upper:]' '[:lower:]')
    case "$mach" in
        arm64)
            if [[ "$out" != *"arm64"* ]]; then
                log "${RED}此 Mac 的 gpuf-c 二进制架构无效（需要 arm64）${NC}"
                log "${YELLOW}$out${NC}"
                return 1
            fi
            ;;
        x86_64)
            if [[ "$out" != *"x86_64"* && "$out" != *"x86-64"* ]]; then
                log "${RED}此 Mac 的 gpuf-c 二进制架构无效（需要 x86_64）${NC}"
                log "${YELLOW}$out${NC}"
                return 1
            fi
            ;;
        *)
            # Unknown arch; allow Mach-O only
            ;;
    esac
}

# detect os and architecture
# NOTE: This installer is for the llama.cpp version of gpuf-c.
# It downloads a compressed release archive and extracts it.
detect_system() {
    echo "=== 系统检测 ==="

    local u
    u="$(uname)"

    case "$u" in
        Darwin)
            OS="darwin"
            ARCH="$(uname -m)"
            echo "操作系统：macOS ($ARCH)"
            ;;
        Linux)
            OS="linux"
            ARCH="$(uname -m)"
            echo "操作系统：Linux ($ARCH)"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            OS="windows"
            ARCH="x86_64"
            echo "操作系统：Windows (通过 $u)"
            ;;
        *)
            OS="linux"
            ARCH="$(uname -m)"
            echo "操作系统：$u ($ARCH)"
            ;;
    esac

    export OS
    export ARCH
}

# Installation directory
get_install_dir() {
    echo "/usr/local/bin"
}

get_share_dir() {
    echo "/usr/local/share/gpuf-c"
}

normalize_arch() {
    case "$ARCH" in
        x86_64|amd64)
            echo "x86_64"
            ;;
        aarch64|arm64)
            echo "arm64"
            ;;
        *)
            echo "$ARCH"
            ;;
    esac
}

calc_md5() {
    local file="$1"

    if command -v md5sum &> /dev/null; then
        md5sum "$file" | awk '{print $1}'
        return 0
    fi

    if command -v md5 &> /dev/null; then
        md5 -q "$file"
        return 0
    fi

    if command -v openssl &> /dev/null; then
        openssl md5 "$file" | awk '{print $NF}'
        return 0
    fi

    return 1
}

read_md5_hint_file() {
    local md5_file="$1"

    if [ ! -f "$md5_file" ]; then
        return 0
    fi

    # Accept formats like:
    #   <md5>
    #   <md5>  filename
    #   MD5(<file>)= <md5>
    # and also allow short hints for fuzzy match
    local hint
    hint=$(tr -d '\r' < "$md5_file" | head -n 1)
    hint=$(echo "$hint" | sed -E 's/.*=\s*//')
    hint=$(echo "$hint" | awk '{print $1}')
    hint=$(echo "$hint" | tr '[:upper:]' '[:lower:]')
    echo "$hint"
}

verify_md5_contains_if_needed() {
    local file="$1"
    local hint="$2"

    if [ -z "$hint" ]; then
        return 0
    fi

    if [ ! -f "$file" ]; then
        log "${RED}md5 检查失败：文件未找到：$file${NC}"
        return 1
    fi

    local md5
    if ! md5=$(calc_md5 "$file"); then
        log "${RED}md5 检查失败：md5 工具不可用（需要 md5sum/md5/openssl）${NC}"
        return 1
    fi

    md5=$(echo "$md5" | tr '[:upper:]' '[:lower:]')
    hint=$(echo "$hint" | tr '[:upper:]' '[:lower:]')

    if [[ "$md5" != *"$hint"* ]]; then
        log "${RED}$file 的 md5 不匹配${NC}"
        log "${YELLOW}期望包含：$hint${NC}"
        log "${YELLOW}实际 md5：        $md5${NC}"
        return 1
    fi

    log "${GREEN}md5 匹配正确：$md5${NC}"
}

read_md5_prefix_from_filename() {
    local file_path="$1"
    local base
    base=$(basename "$file_path")

    # Expected format: <6hex>-<rest>
    # Example: 6cb2ba-vulkan-gpuf-c
    if [[ "$base" =~ ^([0-9a-fA-F]{6})- ]]; then
        echo "${BASH_REMATCH[1]}" | tr '[:upper:]' '[:lower:]'
        return 0
    fi

    echo ""
}

verify_md5_prefix_from_filename_if_possible() {
    local file="$1"

    if [ ! -f "$file" ]; then
        log "${RED}md5 检查失败：文件未找到：$file${NC}"
        return 1
    fi

    local prefix
    prefix=$(read_md5_prefix_from_filename "$file")
    if [ -z "$prefix" ]; then
        log "${YELLOW}警告：文件名中未找到 md5 前缀（跳过 md5 前缀检查）：$(basename "$file")${NC}"
        return 0
    fi

    local md5
    if ! md5=$(calc_md5 "$file"); then
        log "${RED}md5 检查失败：md5 工具不可用（需要 md5sum/md5/openssl）${NC}"
        return 1
    fi

    md5=$(echo "$md5" | tr '[:upper:]' '[:lower:]')

    if [ "${md5:0:6}" != "$prefix" ]; then
        log "${RED}$file 的 md5 前缀不匹配${NC}"
        log "${YELLOW}期望前缀：$prefix${NC}"
        log "${YELLOW}实际 md5：      $md5${NC}"
        return 1
    fi

    log "${GREEN}md5 前缀匹配正确：$md5${NC}"
}

verify_md5_prefixes_from_extracted_dir_if_needed() {
    local extracted_dir="$1"
    local md5_hint="$2"

    if [ -n "$md5_hint" ]; then
        return 0
    fi

    if [ "$OS" = "linux" ]; then
        local linux_cuda
        linux_cuda=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-cuda-gpuf-c" | head -n 1)
        local linux_vulkan
        linux_vulkan=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-vulkan-gpuf-c" | head -n 1)

        if [ -n "$linux_vulkan" ] && [ -f "$linux_vulkan" ]; then
            verify_md5_prefix_from_filename_if_possible "$linux_vulkan"
        fi

        if [ -n "$linux_cuda" ] && [ -f "$linux_cuda" ]; then
            verify_md5_prefix_from_filename_if_possible "$linux_cuda"
        fi
    else
        local mac_bin
        mac_bin=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-metal-gpuf-c" | head -n 1)
        if [ -n "$mac_bin" ] && [ -f "$mac_bin" ]; then
            verify_md5_prefix_from_filename_if_possible "$mac_bin"
        fi
    fi
}

ensure_dir() {
    local dir="$1"
    if [ ! -d "$dir" ]; then
        mkdir -p "$dir"
    fi
}

# download helper (curl)
download_file() {
    local url="$1"
    local out="$2"

    log "${YELLOW}下载：$url${NC}"
    # Use curl with progress bar (-#) instead of silent mode
    # -f: fail silently on HTTP errors
    # -L: follow redirects
    # -#: show progress bar
    # -o: output file
    if ! curl -fL# "$url" -o "$out" 2>&1 | tee -a "$LOG_FILE"; then
        log "${RED}下载失败：$url${NC}"
        return 1
    fi
    echo "" # Add newline after progress bar
}

extract_archive() {
    local archive="$1"
    local dest_dir="$2"

    ensure_dir "$dest_dir"

    case "$archive" in
        *.tar.gz|*.tgz)
            check_command tar
            tar -xzf "$archive" -C "$dest_dir" >> "$LOG_FILE" 2>&1
            ;;
        *.zip)
            check_command unzip
            unzip -q "$archive" -d "$dest_dir" >> "$LOG_FILE" 2>&1
            ;;
        *)
            log "${RED}不支持的压缩包格式：$archive${NC}"
            return 1
            ;;
    esac
}

install_from_extracted_dir() {
    local extracted_dir="$1"

    local linux_cuda
    linux_cuda=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-cuda-gpuf-c" | head -n 1)
    local linux_vulkan
    linux_vulkan=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-vulkan-gpuf-c" | head -n 1)
    local mac_bin
    mac_bin=$(find "$extracted_dir" -maxdepth 1 -type f -name "*-metal-gpuf-c" | head -n 1)

    if [ "$OS" = "linux" ]; then
        if [ -z "$linux_cuda" ] && [ -z "$linux_vulkan" ]; then
            log "${RED}在解压目录中未找到 Linux 二进制文件：$extracted_dir${NC}"
            return 1
        fi

        if [ -n "$linux_vulkan" ] && [ -f "$linux_vulkan" ]; then
            sudo install -m 0755 "$linux_vulkan" "$INSTALL_DIR/gpuf-c-vulkan" >> "$LOG_FILE" 2>&1
            log "${GREEN}已安装：$INSTALL_DIR/gpuf-c-vulkan${NC}"
        fi

        if [ -n "$linux_cuda" ] && [ -f "$linux_cuda" ]; then
            sudo install -m 0755 "$linux_cuda" "$INSTALL_DIR/gpuf-c-cuda" >> "$LOG_FILE" 2>&1
            log "${GREEN}已安装：$INSTALL_DIR/gpuf-c-cuda${NC}"
        fi

        if command -v nvidia-smi &> /dev/null && [ -f "$INSTALL_DIR/gpuf-c-cuda" ]; then
            sudo ln -sf "$INSTALL_DIR/gpuf-c-cuda" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
            log "${GREEN}已安装：$INSTALL_DIR/gpuf-c (CUDA)${NC}"
        elif command -v vulkaninfo &> /dev/null && [ -f "$INSTALL_DIR/gpuf-c-vulkan" ]; then
            sudo ln -sf "$INSTALL_DIR/gpuf-c-vulkan" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
            log "${GREEN}已安装：$INSTALL_DIR/gpuf-c (Vulkan)${NC}"
        elif [ -f "$INSTALL_DIR/gpuf-c-cuda" ]; then
            sudo ln -sf "$INSTALL_DIR/gpuf-c-cuda" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
            log "${GREEN}已安装：$INSTALL_DIR/gpuf-c (CUDA)${NC}"
        elif [ -f "$INSTALL_DIR/gpuf-c-vulkan" ]; then
            sudo ln -sf "$INSTALL_DIR/gpuf-c-vulkan" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
            log "${GREEN}已安装：$INSTALL_DIR/gpuf-c (Vulkan)${NC}"
        else
            log "${RED}选择默认 gpuf-c 二进制文件失败${NC}"
            return 1
        fi
    else
        if [ -z "$mac_bin" ]; then
            log "${RED}在解压目录中未找到 Mac 二进制文件：$extracted_dir${NC}"
            return 1
        fi

        verify_macos_binary_format "$mac_bin"
        verify_md5_prefix_from_filename_if_possible "$mac_bin"

        sudo install -m 0755 "$mac_bin" "$INSTALL_DIR/gpuf-c" >> "$LOG_FILE" 2>&1
        log "${GREEN}已安装：$INSTALL_DIR/gpuf-c${NC}"
    fi

    if [ -f "$extracted_dir/read.txt" ]; then
        local share_dir
        share_dir=$(get_share_dir)
        sudo mkdir -p "$share_dir" >> "$LOG_FILE" 2>&1
        sudo install -m 0644 "$extracted_dir/read.txt" "$share_dir/read.txt" >> "$LOG_FILE" 2>&1
        log "${GREEN}已安装：$share_dir/read.txt${NC}"
    fi

    if [ -f "$extracted_dir/ca-cert.pem" ]; then
        sudo install -m 0644 "$extracted_dir/ca-cert.pem" "$INSTALL_DIR/ca-cert.pem" >> "$LOG_FILE" 2>&1
        log "${GREEN}已安装：$INSTALL_DIR/ca-cert.pem${NC}"
    fi
}

verify_installation() {
    log "${GREEN}=== 安装完成 ===${NC}"
    log "${YELLOW}验证安装：${NC}"

    local share_dir
    share_dir=$(get_share_dir)
    if [ -f "$share_dir/read.txt" ]; then
        log "${YELLOW}使用指南：${NC}"
        log "  ${GREEN}$share_dir/read.txt${NC}"
    fi

    if command -v gpuf-c &> /dev/null; then
        log "${GREEN}✓ gpuf-c 安装成功${NC}"
        gpuf-c --version 2>/dev/null || true
    else
        log "${RED}✗ gpuf-c 安装失败${NC}"
    fi
}

# main install function
main() {
    log "${YELLOW}=== gpuf-c (llama.cpp) 安装过程 ===${NC}"

    detect_system

    INSTALL_DIR=$(get_install_dir)

    check_command "curl"

    case "$OS" in
        linux|darwin)
            check_command sudo

            local arch_norm
            arch_norm=$(normalize_arch)

            BASE_URL="${GPUF_C_CLIENT_BASE_URL:-https://oss.gpunexus.com/client}"

            local pkg_os
            if [ "$OS" = "darwin" ]; then
                pkg_os="mac"
            else
                pkg_os="$OS"
            fi

            local archive_name
            archive_name="v1.0.2-${pkg_os}-gpuf-c.tar.gz"

            ARCHIVE_NAME="${GPUF_C_CLIENT_ARCHIVE_NAME:-$archive_name}"

            local tmp_dir
            tmp_dir=$(mktemp -d)
            local archive_path="$tmp_dir/$ARCHIVE_NAME"
            local extract_dir="$tmp_dir/extract"

            if ! download_file "$BASE_URL/$ARCHIVE_NAME" "$archive_path"; then
                exit 1
            fi
            extract_archive "$archive_path" "$extract_dir"

            local payload
            payload="$extract_dir"
            if [ ! -d "$payload" ]; then
                log "${RED}未能定位解压后的内容${NC}"
                exit 1
            fi

            local top
            top=$(find "$payload" -maxdepth 1 -type d ! -path "$payload" | head -n 1)
            if [ -n "$top" ] && [ -f "$top/read.txt" ]; then
                payload="$top"
            fi

            verify_md5_prefixes_from_extracted_dir_if_needed "$payload" ""

            if [ "$OS" = "linux" ]; then
                if command -v nvidia-smi &> /dev/null; then
                    log "${GREEN}检测到：NVIDIA (CUDA)${NC}"
                elif command -v vulkaninfo &> /dev/null; then
                    log "${GREEN}检测到：Vulkan 运行时${NC}"
                else
                    log "${RED}错误：Linux 需要 nvidia-smi (CUDA) 或 vulkaninfo (Vulkan 运行时)${NC}"
                    exit 1
                fi
            fi

            install_from_extracted_dir "$payload"

            rm -rf "$tmp_dir"
            ;;
        *)
            log "${RED}不支持操作系统：$OS${NC}"
            exit 1
            ;;
    esac

    verify_installation
}

main "$@"
