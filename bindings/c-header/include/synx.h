#ifndef SYNX_H
#define SYNX_H

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
 * Free a string returned by any synx_* function.
 * Passing NULL is allowed.
 */
void synx_free(char* ptr);

#ifdef __cplusplus
}
#endif

#endif /* SYNX_H */
