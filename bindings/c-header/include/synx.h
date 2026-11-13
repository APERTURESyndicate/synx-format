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
 * - On error, functions return NULL. A failure inside the library never
 *   unwinds or aborts into the caller: it is reported as NULL (or 0).
 *
 * Input limits (the parser drops what lies past them, without an error):
 * documents up to 16 MiB, nesting up to 128 levels, a multiline block up to
 * 1 MiB, a list up to 1 048 576 items.
 *
 * Secrets: a `:secret` value is written as "[SECRET]" in every JSON this
 * library returns.
 */

/**
 * Parse a SYNX string and return a JSON string.
 * Returns NULL on invalid input or internal error.
 * Caller must free the result with synx_free().
 */
char* synx_parse(const char* input);

/**
 * Parse a SYNX string with engine resolution (!active mode) and return JSON.
 * `:env` reads the environment of this process, and `:include` / `:watch` /
 * `!use` / the `:once` lock file use the current directory. For a document
 * you did not write, use synx_parse_active_ex().
 * Returns NULL on invalid input or internal error.
 * Caller must free the result with synx_free().
 */
char* synx_parse_active(const char* input);

/**
 * synx_parse_active() with the environment and the base directory chosen by
 * the caller.
 *
 * env_json  — JSON object of strings, e.g. {"PORT":"8080"}: the only
 *             variables `:env` can see. "{}" gives the document none.
 *             NULL: the environment of this process.
 * base_path — directory that `:include`, `:watch`, `!use` and the `:once`
 *             lock file resolve in and cannot leave. NULL: current directory.
 *
 * Returns NULL on invalid input, when env_json is not an object of strings,
 * or on internal error. Caller must free the result with synx_free().
 */
char* synx_parse_active_ex(const char* input, const char* env_json, const char* base_path);

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
 * An !active document is resolved as in synx_parse_active().
 * Caller must free the result with synx_free().
 */
char* synx_parse_tool(const char* input);

/**
 * synx_parse_tool() with the environment and the base directory chosen by
 * the caller; see synx_parse_active_ex() for the two arguments.
 */
char* synx_parse_tool_ex(const char* input, const char* env_json, const char* base_path);

/**
 * Free a string returned by any synx_* function.
 * Passing NULL is allowed.
 */
void synx_free(char* ptr);

/**
 * Free a byte buffer returned by synx_compile().
 * `len` is the value synx_compile() stored in *out_len.
 */
void synx_free_bytes(unsigned char* ptr, size_t len);

/**
 * Compile a SYNX string into compact binary .synxb format.
 * `resolved`: 0 keeps markers and metadata, any other value resolves the
 * document first.
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
 * Returns 1 if the data is .synxb, 0 otherwise.
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
