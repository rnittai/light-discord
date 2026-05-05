#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname -s)" != "Linux" ]; then
    echo "This script is for Linux only. On other platforms install ALSA dev headers manually."
    exit 0
fi

# We need ALSA dev headers (cpal capture/playback), xcap dependencies for screen
# capture, AND cmake + a C toolchain (audiopus_sys builds libopus from source via
# CMake when the `static` feature is enabled). pkg-config helps cargo locate ALSA.
# If everything is already present we have nothing to do.
required_pc_modules="alsa dbus-1 libpipewire-0.3 wayland-client egl gbm xcb xrandr"

need_install=0
if ! command -v pkg-config >/dev/null 2>&1; then
    need_install=1
else
    for module in $required_pc_modules; do
        if ! pkg-config --exists "$module" 2>/dev/null; then
            need_install=1
            break
        fi
    done
fi
if ! command -v cmake >/dev/null 2>&1; then
    need_install=1
fi
if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1; then
    need_install=1
fi
if [ "$need_install" -eq 0 ]; then
    echo "ALSA/xcap pkg-config entries found and cmake / C toolchain present."
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

echo "Installing ALSA, xcap, and libopus build dependencies..."

case "$family" in
    debian)
        if ! command -v apt-get >/dev/null 2>&1; then
            echo "Error: apt-get not found on a Debian-family system."
            exit 1
        fi
        run_install apt-get update
        run_install apt-get install -y pkg-config libasound2-dev cmake build-essential libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev
        ;;
    fedora)
        if command -v dnf >/dev/null 2>&1; then
            run_install dnf install -y pkgconf-pkg-config alsa-lib-devel cmake gcc gcc-c++ make clang-devel libxcb-devel libXrandr-devel dbus-devel pipewire-devel wayland-devel mesa-libEGL-devel mesa-libgbm-devel libxkbcommon-devel
        elif command -v yum >/dev/null 2>&1; then
            run_install yum install -y pkgconf-pkg-config alsa-lib-devel cmake gcc gcc-c++ make clang-devel libxcb-devel libXrandr-devel dbus-devel pipewire-devel wayland-devel mesa-libEGL-devel mesa-libgbm-devel libxkbcommon-devel
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
        run_install pacman -Sy --needed --noconfirm pkgconf alsa-lib cmake base-devel clang libxcb libxrandr dbus pipewire wayland mesa libxkbcommon
        ;;
    suse)
        if ! command -v zypper >/dev/null 2>&1; then
            echo "Error: zypper not found on an openSUSE-family system."
            exit 1
        fi
        run_install zypper --non-interactive install pkgconf-pkg-config alsa-devel cmake gcc gcc-c++ make clang-devel libxcb-devel libXrandr-devel dbus-1-devel pipewire-devel wayland-devel Mesa-libEGL-devel Mesa-libgbm-devel libxkbcommon-devel
        ;;
    *)
        echo "Unsupported distro (ID='$ID' ID_LIKE='$ID_LIKE')."
        echo "Install pkg-config, ALSA development headers, X11/Wayland development"
        echo "headers, dbus, pipewire, cmake, and a C/C++ toolchain manually, then re-run."
        exit 1
        ;;
esac

# Verify the installation worked.
for module in $required_pc_modules; do
    if ! pkg-config --exists "$module" 2>/dev/null; then
        echo "Error: pkg-config still cannot find $module after installation."
        echo "PKG_CONFIG_PATH=${PKG_CONFIG_PATH:-}"
        echo "pkg-config search path:"
        pkg-config --variable pc_path pkg-config 2>/dev/null || true
        exit 1
    fi
done
if ! command -v cmake >/dev/null 2>&1; then
    echo "Error: cmake is still missing after installation."
    exit 1
fi
if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1; then
    echo "Error: a C compiler (cc/gcc) is still missing after installation."
    exit 1
fi

echo "Done. ALSA, xcap, cmake, and C toolchain dependencies are ready."
echo "  CFLAGS : $(pkg-config --cflags alsa)"
echo "  LIBS   : $(pkg-config --libs alsa)"
echo "  cmake  : $(cmake --version | head -1)"
