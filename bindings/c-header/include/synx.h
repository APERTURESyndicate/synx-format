#ifndef SYNX_H
#define SYNX_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Memory ownership contract:
 * - Every non-NULL `char*` returned by a `synx_*` function is heap-allocated.
 * - The caller must release it exactly once via `synx_free()`.
 * - On error, functions return NULL.
 */

/**
 * Parse a SYNX string and return a JSON string.
 * Returns NULL on invalid input or internal error.
 * Caller must free the result with synx_free().
 */
char* synx_parse(const char* input);

/**
 * Parse a SYNX string with engine resolution (!active mode) and return JSON.
 * Returns NULL on invalid input or internal error.
 * Caller must free the result with synx_free().
 */
char* synx_parse_active(const char* input);

/**
 * Convert a JSON string back to SYNX format text.
 * Returns NULL if `json_input` is not valid UTF-8 JSON.
 * Caller must free the result with synx_free().
 */
char* synx_stringify(const char* json_input);

/**
 * Reformat a SYNX string into canonical form (sorted, normalized).
 * Returns NULL on invalid input or internal error.
 * Caller must free the result with synx_free().
 */
char* synx_format(const char* input);

/**
 * Parse a !tool SYNX string and return reshaped JSON.
 * Call mode: { "tool": "name", "params": { ... } }
 * Schema mode (!tool + !schema): { "tools": [ ... ] }
 * Caller must free the result with synx_free().
 */
char* synx_parse_tool(const char* input);

/**
 * Free a string returned by any synx_* function.
 * Passing NULL is allowed.
 */
void synx_free(char* ptr);

/**
 * Free a byte buffer returned by synx_compile().
 */
void synx_free_bytes(unsigned char* ptr, size_t len);

/**
 * Compile a SYNX string into compact binary .synxb format.
 * Sets *out_len to the byte count.
 * Caller must free the result with synx_free_bytes().
 * Returns NULL on error.
 */
unsigned char* synx_compile(const char* input, int resolved, size_t* out_len);

/**
 * Decompile a .synxb binary back into a SYNX string.
 * Caller must free the result with synx_free().
 * Returns NULL on error.
 */
char* synx_decompile(const unsigned char* data, size_t len);

/**
 * Check whether the given bytes start with the .synxb magic header.
 * Returns non-zero (true) if the data is .synxb.
 */
int synx_is_synxb(const unsigned char* data, size_t len);

/**
 * Structural diff between two SYNX strings. Returns JSON:
 * { "added": {...}, "removed": {...}, "changed": {...}, "unchanged": [...] }
 * Caller must free the result with synx_free().
 * Returns NULL on error.
 */
char* synx_diff(const char* input_a, const char* input_b);

#ifdef __cplusplus
}
#endif

#endif /* SYNX_H */
