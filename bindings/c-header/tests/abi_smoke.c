/*
 * Compiles synx.h against the built library and checks the parts of the ABI
 * a Rust-only test cannot see: what a C caller reads from `int` results and
 * passes for `int` arguments.
 *
 *   cargo build -p synx-c
 *   cc -I bindings/c-header/include bindings/c-header/tests/abi_smoke.c \
 *      -L target/debug -lsynx_c -o abi_smoke && ./abi_smoke
 *
 * Exit code 0 = every check passed.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "synx.h"

static int failures = 0;

static void check(int ok, const char* what) {
    if (!ok) {
        failures++;
        fprintf(stderr, "FAIL: %s\n", what);
    }
}

/* Leaves garbage in the upper bytes of the result register before the call. */
static volatile long long scratch_register = 0x584e5900584e5900LL;

int main(void) {
    const unsigned char text[] = "hello world, definitely not a binary";
    size_t len = 0;
    unsigned char* bin;
    char* json;
    char* back;
    int i;

    /* synx_is_synxb: the whole int is the answer. */
    for (i = 0; i < 4; i++) {
        long long noise = scratch_register * (i + 3);
        (void)noise;
        check(synx_is_synxb(text, sizeof text - 1) == 0, "synx_is_synxb(text) == 0");
    }
    check(synx_is_synxb(NULL, 10) == 0, "synx_is_synxb(NULL) == 0");

    bin = synx_compile("k v\n", 0, &len);
    check(bin != NULL && len > 5, "synx_compile returns bytes");
    if (bin != NULL) {
        check(synx_is_synxb(bin, len) == 1, "synx_is_synxb(compiled) == 1");
        back = synx_decompile(bin, len);
        check(back != NULL && strstr(back, "k") != NULL, "synx_decompile round trip");
        synx_free(back);
        synx_free_bytes(bin, len);
    }

    /* synx_compile: `int resolved`, any non-zero value resolves. */
    {
        const char* doc = "!active\nport:env:default:8080 SYNX_ABI_SMOKE_UNSET\n";
        size_t len0 = 0, len1 = 0, len2 = 0, len255 = 0;
        unsigned char* b0 = synx_compile(doc, 0, &len0);
        unsigned char* b1 = synx_compile(doc, 1, &len1);
        unsigned char* b2 = synx_compile(doc, 2, &len2);
        unsigned char* b255 = synx_compile(doc, 255, &len255);
        check(b0 && b1 && b2 && b255, "synx_compile with resolved 0/1/2/255");
        if (b0 && b1 && b2 && b255) {
            check(len0 != len1 || memcmp(b0, b1, len0) != 0, "resolved changes the output");
            check(len2 == len1 && memcmp(b2, b1, len1) == 0, "resolved=2 equals resolved=1");
            check(len255 == len1 && memcmp(b255, b1, len1) == 0, "resolved=255 equals resolved=1");
        }
        synx_free_bytes(b0, len0);
        synx_free_bytes(b1, len1);
        synx_free_bytes(b2, len2);
        synx_free_bytes(b255, len255);
    }

    /* synx_parse_active_ex: the document sees only the environment it is handed. */
    json = synx_parse_active_ex("!active\nport:env:default:8080 PATH\n", "{}", NULL);
    check(json != NULL && strstr(json, "\"port\":8080") != NULL, "empty env hides PATH");
    synx_free(json);
    json = synx_parse_active_ex("!active\nname:env WHO\n", "{\"WHO\":\"abi\"}", NULL);
    check(json != NULL && strstr(json, "\"name\":\"abi\"") != NULL, "env_json is used");
    synx_free(json);
    check(synx_parse_active_ex("!active\na 1\n", "not json", NULL) == NULL, "bad env_json is NULL");

    json = synx_parse_tool_ex("!tool\nsearch\n  query x\n", "{}", NULL);
    check(json != NULL && strstr(json, "\"tool\":\"search\"") != NULL, "synx_parse_tool_ex");
    synx_free(json);

    /* Errors are NULL, never a crash. */
    check(synx_parse(NULL) == NULL, "synx_parse(NULL)");
    check(synx_stringify("not json {{{") == NULL, "synx_stringify(bad json)");
    synx_free(NULL);
    synx_free_bytes(NULL, 0);

    if (failures == 0) {
        printf("abi_smoke: ok\n");
        return 0;
    }
    printf("abi_smoke: %d failure(s)\n", failures);
    return 1;
}
