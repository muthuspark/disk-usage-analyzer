#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Some Linux installations have the runtime library but no development symlink.
# Keep the fallback inside the ignored build directory; never alter system files.
if [[ "$(uname -s)" == Linux ]] && [[ "$(cc -print-file-name=libxkbcommon-x11.so)" == libxkbcommon-x11.so ]]; then
    native_dir="$PWD/target/native-libs"
    runtime_lib="$(ldconfig -p 2>/dev/null | awk '/libxkbcommon-x11\.so\.0 .*x86-64/ && !found { print $NF; found=1 }')"
    if [[ -n "$runtime_lib" ]]; then
        mkdir -p "$native_dir"
        ln -sf "$runtime_lib" "$native_dir/libxkbcommon-x11.so"
        export LIBRARY_PATH="$native_dir${LIBRARY_PATH:+:$LIBRARY_PATH}"
    fi
fi

exec cargo run --locked "$@"
