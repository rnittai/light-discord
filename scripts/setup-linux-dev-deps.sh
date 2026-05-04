#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname -s)" != "Linux" ]; then
    echo "This script is for Linux only. On other platforms install ALSA dev headers manually."
    exit 0
fi

# We need ALSA dev headers (cpal capture/playback) AND cmake + a C toolchain
# (audiopus_sys builds libopus from source via CMake when the `static` feature
# is enabled). pkg-config helps cargo locate ALSA. If everything is already
# present we have nothing to do.
need_install=0
if ! command -v pkg-config >/dev/null 2>&1 || ! pkg-config --exists alsa 2>/dev/null; then
    need_install=1
fi
if ! command -v cmake >/dev/null 2>&1; then
    need_install=1
fi
if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1; then
    need_install=1
fi
if [ "$need_install" -eq 0 ]; then
    echo "ALSA pkg-config entry found and cmake / C toolchain present."
    echo "  CFLAGS : $(pkg-config --cflags alsa)"
    echo "  LIBS   : $(pkg-config --libs alsa)"
    echo "  cmake  : $(cmake --version | head -1)"
    exit 0
fi

# Detect distro from /etc/os-release.
ID=""
ID_LIKE=""
if [ -f /etc/os-release ]; then
    # shellcheck source=/dev/null
    . /etc/os-release
fi

# Classify the distro family.
distro_family() {
    local id="${1:-}" id_like="${2:-}"
    for token in $id $id_like; do
        case "$token" in
            debian|ubuntu|linuxmint|pop|elementary|kali|raspbian)
                echo debian; return ;;
            fedora|rhel|centos|rocky|alma|almalinux|ol)
                echo fedora; return ;;
            arch|manjaro|endeavouros|garuda)
                echo arch; return ;;
            opensuse*|sles|suse)
                echo suse; return ;;
        esac
    done
    echo unknown
}

family=$(distro_family "$ID" "$ID_LIKE")

# Run a command with sudo when not root.
run_install() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    else
        if ! command -v sudo >/dev/null 2>&1; then
            echo "Error: not running as root and sudo is not available."
            echo "Run as root or install sudo, then re-run this script."
            exit 1
        fi
        sudo "$@"
    fi
}

echo "Installing ALSA + libopus build dependencies..."

case "$family" in
    debian)
        if ! command -v apt-get >/dev/null 2>&1; then
            echo "Error: apt-get not found on a Debian-family system."
            exit 1
        fi
        run_install apt-get update
        run_install apt-get install -y pkg-config libasound2-dev cmake build-essential
        ;;
    fedora)
        if command -v dnf >/dev/null 2>&1; then
            run_install dnf install -y pkgconf-pkg-config alsa-lib-devel cmake gcc gcc-c++ make
        elif command -v yum >/dev/null 2>&1; then
            run_install yum install -y pkgconf-pkg-config alsa-lib-devel cmake gcc gcc-c++ make
        else
            echo "Error: neither dnf nor yum found on a Fedora-family system."
            exit 1
        fi
        ;;
    arch)
        if ! command -v pacman >/dev/null 2>&1; then
            echo "Error: pacman not found on an Arch-family system."
            exit 1
        fi
        run_install pacman -Sy --needed --noconfirm pkgconf alsa-lib cmake base-devel
        ;;
    suse)
        if ! command -v zypper >/dev/null 2>&1; then
            echo "Error: zypper not found on an openSUSE-family system."
            exit 1
        fi
        run_install zypper --non-interactive install pkgconf-pkg-config alsa-devel cmake gcc gcc-c++ make
        ;;
    *)
        echo "Unsupported distro (ID='$ID' ID_LIKE='$ID_LIKE')."
        echo "Install pkg-config, ALSA development headers, cmake, and a C/C++"
        echo "toolchain manually, then re-run."
        exit 1
        ;;
esac

# Verify the installation worked.
if ! pkg-config --exists alsa 2>/dev/null; then
    echo "Error: pkg-config still cannot find alsa after installation."
    echo "PKG_CONFIG_PATH=${PKG_CONFIG_PATH:-}"
    echo "pkg-config search path:"
    pkg-config --variable pc_path pkg-config 2>/dev/null || true
    exit 1
fi
if ! command -v cmake >/dev/null 2>&1; then
    echo "Error: cmake is still missing after installation."
    exit 1
fi
if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1; then
    echo "Error: a C compiler (cc/gcc) is still missing after installation."
    exit 1
fi

echo "Done. ALSA development headers, cmake, and C toolchain are ready."
echo "  CFLAGS : $(pkg-config --cflags alsa)"
echo "  LIBS   : $(pkg-config --libs alsa)"
echo "  cmake  : $(cmake --version | head -1)"
