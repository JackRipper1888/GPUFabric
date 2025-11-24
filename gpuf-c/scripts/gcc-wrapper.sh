#!/bin/bash
# Wrapper script to filter out Clang-specific flags that break GCC/MinGW

# The compiler we want to use
REAL_CC="x86_64-w64-mingw32-gcc"
REAL_CXX="x86_64-w64-mingw32-g++"

# Determine if we are being called as CC or CXX
CMD=$REAL_CC
if [[ "$0" == *"g++"* ]]; then
    CMD=$REAL_CXX
fi

# Build new argument list
ARGS=()
for arg in "$@"; do
    case "$arg" in
        -Wno-c11-extensions)
            # Skip this flag
            ;;
        -Wno-unused-command-line-argument)
             # Skip this flag
             ;;
        -Werror)
             # Skip this flag to prevent warnings (like macro redefinition) from breaking the build
             ;;
        *)
            ARGS+=("$arg")
            ;;
    esac
done

# Execute the real compiler
exec "$CMD" "${ARGS[@]}"
