#ifndef MINGW_FIX_H
#define MINGW_FIX_H

#ifdef _WIN32

// Do NOT include windows.h here as it pollutes the namespace for other crates (like aws-lc)
// #include <windows.h>

// Fix for missing THREAD_POWER_THROTTLING_STATE in older MinGW headers
// Structure and constants taken from Microsoft docs
#ifndef THREAD_POWER_THROTTLING_CURRENT_VERSION

// Define ULONG if not available, but usually it is if windows.h is included by the source file.
// However, since we are injecting this, we can't be sure.
// To avoid conflict, we rely on the fact that llama.cpp likely included windows.h before usage.
// But if we define the struct, we need the types.
// Let's use 'unsigned long' which is standard.

typedef struct _THREAD_POWER_THROTTLING_STATE {
  unsigned long Version;
  unsigned long ControlMask;
  unsigned long StateMask;
} THREAD_POWER_THROTTLING_STATE;

#define THREAD_POWER_THROTTLING_CURRENT_VERSION 1
#define THREAD_POWER_THROTTLING_EXECUTION_SPEED 0x1

#endif // THREAD_POWER_THROTTLING_CURRENT_VERSION
#endif // _WIN32

#endif // MINGW_FIX_H
