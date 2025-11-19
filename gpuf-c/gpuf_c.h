#ifndef GPUF_C_H
#define GPUF_C_H

#pragma once

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Initialize GPUFabric library
 * Returns: 0 for success, -1 for failure
 */
int32_t gpuf_init(void);

/**
 * Get last error information
 * Returns: Error message string pointer, caller needs to call gpuf_free_string to release
 */
char *gpuf_get_last_error(void);

/**
 * Release string allocated by the library
 */
void gpuf_free_string(char *s);

/**
 * Get version information
 */
const char *gpuf_version(void);

/**
 * Initialize LLM engine - Not supported in lightweight version
 */
int32_t gpuf_llm_init(const char *_model_path, uint32_t _n_ctx, uint32_t _n_gpu_layers);

/**
 * Generate text - Not supported in lightweight version
 */
char *gpuf_llm_generate(const char *_prompt, uintptr_t _max_tokens);

/**
 * Check if LLM engine is initialized - Always false in lightweight version
 */
int32_t gpuf_llm_is_initialized(void);

/**
 * Unload LLM engine - No-op in lightweight version
 */
int32_t gpuf_llm_unload(void);

#endif /* GPUF_C_H */
