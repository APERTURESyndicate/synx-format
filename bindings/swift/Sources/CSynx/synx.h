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

char* synx_parse(const char* input);
char* synx_parse_active(const char* input);
char* synx_stringify(const char* json_input);
char* synx_format(const char* input);
char* synx_parse_tool(const char* input);
void synx_free(char* ptr);
void synx_free_bytes(unsigned char* ptr, size_t len);
unsigned char* synx_compile(const char* input, int resolved, size_t* out_len);
char* synx_decompile(const unsigned char* data, size_t len);
int synx_is_synxb(const unsigned char* data, size_t len);
char* synx_diff(const char* input_a, const char* input_b);

#ifdef __cplusplus
}
#endif

#endif /* SYNX_H */
