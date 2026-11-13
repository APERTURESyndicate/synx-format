package com.aperturesyndicate.synx;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.zip.Deflater;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTimeoutPreemptively;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Inputs that used to print a secret, kill the thread, hang, or eat the heap. */
class HardeningTest {

    private static SynxOptions noEnv(Path base) {
        SynxOptions o = new SynxOptions();
        o.env = new HashMap<>();
        if (base != null) o.basePath = base.toString();
        return o;
    }

    private static String active(String text) {
        return Synx.toJson(Synx.parseActive(text, noEnv(null)));
    }

    // ─── secrets ────────────────────────────────────────────────────────────

    @Test
    void a_secret_is_never_printed() {
        SynxObject obj = Synx.parseActive("!active\npassword:secret hunter2\ndb\n  token:secret abc123\n", noEnv(null));
        SynxValue secret = obj.get("password");
        assertTrue(secret.isSecret());
        assertEquals("hunter2", secret.asSecret());

        for (String shown : new String[] {
                secret.toString(), String.valueOf(secret), "" + secret, String.format("%s", secret),
                obj.toString(), SynxValue.ofObject(obj).toString(), java.util.List.of(secret).toString(),
                java.util.Objects.toString(obj.get("db")), new IllegalStateException("bad value: " + secret).getMessage() }) {
            assertFalse(shown.contains("hunter2"), shown);
            assertFalse(shown.contains("abc123"), shown);
        }
        assertEquals("[SECRET]", secret.toString());
        assertFalse(Synx.toJson(obj).contains("hunter2"));
        assertFalse(Synx.stringify(obj).contains("hunter2"));
    }

    @Test
    void an_unset_secret_is_null_and_required_fires() {
        assertEquals(
            "{\"a\":null,\"b\":\"CONSTRAINT_ERR: 'b' is required\",\"lit\":\"[SECRET]\"}",
            active("!active\na:secret:env SYNX_TEST_UNSET\nb[required]:secret:env SYNX_TEST_UNSET\nlit:secret x\n"));
    }

    @Test
    void a_required_key_without_a_value_is_missing() {
        assertEquals("{\"name\":\"CONSTRAINT_ERR: 'name' is required\"}", active("!active\nname[required, min:1]\n"));
        // A group with fields is a value.
        assertEquals("{\"g\":{\"x\":1}}", active("!active\ng[required]\n  x 1\n"));
    }

    // ─── :calc ──────────────────────────────────────────────────────────────

    @Test
    void deep_parentheses_do_not_overflow_the_stack() throws Exception {
        String deep = "(".repeat(200_000) + "1" + ")".repeat(200_000);
        Throwable[] thrown = { null };
        double[] value = { 0 };
        // A small stack: the recursive evaluator died on about 1 500 parentheses here.
        Thread t = new Thread(null, () -> {
            try {
                SynxCalc.Result r = SynxCalc.evaluate(deep);
                value[0] = r.ok ? r.value : Double.NaN;
            } catch (Throwable e) {
                thrown[0] = e;
            }
        }, "calc", 256 * 1024);
        t.start();
        t.join();
        assertEquals(null, thrown[0]);
        assertEquals(1.0, value[0]);

        assertEquals("SYNX :calc — unexpected end of expression", SynxCalc.evaluate("(".repeat(50_000)).error);
        assertEquals("SYNX :calc — missing closing parenthesis", SynxCalc.evaluate("(".repeat(50_000) + "1").error);
        assertEquals("SYNX :calc — missing closing parenthesis", SynxCalc.evaluate("(1 2").error);
        String doc = active("!active\nbase 5\nx:ref:calc:+" + "(".repeat(20_000) + "1" + ")".repeat(20_000) + " base\n");
        assertTrue(doc.contains("\"x\":6"), doc.substring(0, Math.min(doc.length(), 200)));
    }

    @Test
    void the_iterative_evaluator_keeps_every_result_and_error() {
        Object[][] cases = {
            { "2 + 3 * 4", 14.0 }, { "(2 + 3) * 4", 20.0 }, { "10 / 4", 2.5 }, { "7 % 3", 1.0 }, { "-7 % 3", -1.0 },
            { "2 * (3 + (4 - 1)) / 3", 4.0 }, { "-(2)", null }, { "1 - -1", 2.0 }, { "((1))", 1.0 }, { "", 0.0 },
            { "1 +", null }, { "1 / 0", null }, { "5 % 0", null }, { "(1", null }, { "1)", null }, { ")", null },
            { "1 2", null }, { "* 2", null }, { "2 * * 3", null }, { "()", null },
        };
        String[] errors = {
            null, null, null, null, null, null, "SYNX :calc — invalid number: '-'", null, null, null,
            "SYNX :calc — unexpected end of expression", "SYNX :calc — division by zero", "SYNX :calc — division by zero",
            "SYNX :calc — missing closing parenthesis", "SYNX :calc — unexpected token at position 1",
            "SYNX :calc — unexpected token: ')'", "SYNX :calc — unexpected token at position 1",
            "SYNX :calc — unexpected token: '*'", "SYNX :calc — unexpected token: '*'", "SYNX :calc — unexpected token: ')'",
        };
        for (int i = 0; i < cases.length; i++) {
            SynxCalc.Result r = SynxCalc.evaluate((String) cases[i][0]);
            if (cases[i][1] != null) {
                assertTrue(r.ok, cases[i][0] + " → " + r.error);
                assertEquals((Double) cases[i][1], r.value, (String) cases[i][0]);
            } else {
                assertFalse(r.ok, (String) cases[i][0]);
                assertEquals(errors[i], r.error, (String) cases[i][0]);
            }
        }
    }

    // ─── includes ───────────────────────────────────────────────────────────

    @Test
    void a_file_that_includes_itself_three_times_is_cut_at_the_cycle(@TempDir Path dir) throws Exception {
        String text = "!active\na:include bomb.synx\nb:include bomb.synx\nc:include bomb.synx\n";
        Files.writeString(dir.resolve("bomb.synx"), text);
        String json = assertTimeoutPreemptively(java.time.Duration.ofSeconds(10),
            () -> Synx.toJson(Synx.parseActive(text, noEnv(dir))));
        assertTrue(json.contains("INCLUDE_ERR: circular include: 'bomb.synx'"), json);

        Files.writeString(dir.resolve("ping.synx"), "!active\na:include pong.synx\n");
        Files.writeString(dir.resolve("pong.synx"), "!active\nb:include ping.synx\n");
        assertEquals("{\"a\":{\"b\":{\"a\":\"INCLUDE_ERR: circular include: 'pong.synx'\"}}}",
            Synx.toJson(Synx.parseActive(Files.readString(dir.resolve("ping.synx")), noEnv(dir))));

        // The same file twice, one after the other, is not a cycle.
        Files.writeString(dir.resolve("part.synx"), "v 1\n");
        assertEquals("{\"x\":{\"v\":1},\"y\":{\"v\":1}}",
            Synx.toJson(Synx.parseActive("!active\nx:include part.synx\ny:include part.synx\n", noEnv(dir))));
    }

    @Test
    void file_markers_never_open_a_device_or_print_the_host_path(@TempDir Path dir) throws Exception {
        for (String device : new String[] { "NUL", "nul", "con", "CON.txt", "sub/COM1", "lpt9.log", "aux", "PRN" }) {
            assertTrue(SynxEngine.hasDeviceName(Path.of(device)), device);
        }
        for (String plain : new String[] { "null.synx", "console.synx", "com.synx", "sub/part.synx", "COM10" }) {
            assertFalse(SynxEngine.hasDeviceName(Path.of(plain)), plain);
        }
        String json = Synx.toJson(Synx.parseActive(
            "!active\na:include " + "x".repeat(300) + ".synx\nb:watch " + "y".repeat(300) + ".json\nc:include missing.synx\n",
            noEnv(dir)));
        assertFalse(json.contains(dir.getFileName().toString()), json);
        assertTrue(json.contains("\"c\":\"INCLUDE_ERR: file not found: missing.synx\""), json);

        // A base directory that does not exist jails nothing, so nothing is read through it.
        SynxOptions gone = noEnv(dir.resolve("no-such-dir"));
        String refused = Synx.toJson(Synx.parseActive("!active\nx:include part.synx\n", gone));
        assertTrue(refused.contains("INCLUDE_ERR: SECURITY: base directory not found"), refused);
    }

    // ─── .synxb ─────────────────────────────────────────────────────────────

    private static byte[] varint(long n) {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        while (true) {
            int b = (int) (n & 0x7F);
            n >>>= 7;
            if (n == 0) { out.write(b); return out.toByteArray(); }
            out.write(b | 0x80);
        }
    }

    private static byte[] container(byte[] payload, long declared) {
        Deflater deflater = new Deflater(9, true);
        deflater.setInput(payload);
        deflater.finish();
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        out.writeBytes("SYNXB".getBytes(StandardCharsets.US_ASCII));
        out.write(1);
        out.write(0);
        for (int i = 0; i < 4; i++) out.write((int) ((declared >>> (8 * i)) & 0xFF));
        byte[] buf = new byte[65536];
        while (!deflater.finished()) out.write(buf, 0, deflater.deflate(buf));
        deflater.end();
        return out.toByteArray();
    }

    private static byte[] join(byte[]... parts) {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        for (byte[] p : parts) out.writeBytes(p);
        return out.toByteArray();
    }

    private static long usedHeap() {
        Runtime rt = Runtime.getRuntime();
        return rt.totalMemory() - rt.freeMemory();
    }

    @Test
    void a_small_file_cannot_decode_into_a_huge_tree() {
        // 8 388 608 nulls behind a deflate bomb: 8 KB on disk.
        byte[] nulls = new byte[8 * 1024 * 1024];
        byte[] nullsPayload = join(varint(0), new byte[] { 0x06 }, varint(nulls.length), nulls);
        byte[] bomb = container(nullsPayload, nullsPayload.length);
        var res = SynxBinary.decompile(bomb);
        assertFalse(res.ok);
        assertEquals("decoded document exceeds 268435456 bytes — refusing to decode", res.error);

        // One 120 KB string referenced 6 000 times: 171 bytes on disk, 720 MB of text.
        byte[] big = "x".repeat(120_000).getBytes(StandardCharsets.US_ASCII);
        ByteArrayOutputStream refs = new ByteArrayOutputStream();
        for (int i = 0; i < 6000; i++) { refs.write(0x05); refs.write(0x00); }
        byte[] fanoutPayload = join(varint(1), varint(big.length), big, new byte[] { 0x06 }, varint(6000), refs.toByteArray());
        res = SynxBinary.decompile(container(fanoutPayload, fanoutPayload.length));
        assertFalse(res.ok);
        assertEquals("decoded document exceeds 268435456 bytes — refusing to decode", res.error);

        // A count of 2^63 + 5 is not an empty list.
        byte[] huge = join(varint(0), new byte[] { 0x06 }, varint(Long.MIN_VALUE + 5));
        res = SynxBinary.decompile(container(huge, huge.length));
        assertFalse(res.ok, "count 2^63+5 was read as empty");
    }

    @Test
    void the_declared_size_is_not_trusted_for_allocation() {
        // 13 bytes declaring 256 MiB: refused without reserving the buffer first.
        byte[] header = { 'S', 'Y', 'N', 'X', 'B', 1, 0, 0, 0, 0, 0x10, 0x03, 0x00 };
        System.gc();
        long before = usedHeap();
        for (int i = 0; i < 20; i++) assertFalse(SynxBinary.decompile(header).ok);
        assertTrue(usedHeap() - before < 64L * 1024 * 1024, "decoding a 13-byte file allocated " + (usedHeap() - before));

        // A stream longer than it declares stops one buffer past the declared size.
        byte[] zeros = new byte[32 * 1024 * 1024];
        var res = SynxBinary.decompile(container(zeros, 1024));
        assertFalse(res.ok);
        assertTrue(res.error.startsWith("size mismatch: expected 1024"), res.error);

        // A valid file still reads.
        var compiled = Synx.compile("k v\nlist\n  - 1\n  - true\n", false);
        assertTrue(compiled.ok);
        var back = Synx.decompile(compiled.value);
        assertTrue(back.ok, back.error);
        assertTrue(back.value.contains("k v"));
    }

    // B-076: what stands under a refused key goes with it.
    @Test
    void the_block_of_a_refused_key_is_dropped() {
        assertEquals("{\"user\":{\"age\":3,\"profile\":{\"name\":\"x\"}}}",
            Synx.toJson(Synx.parse("user\n  profile\n    name x\n  __proto__\n    role admin\n    name y\n  age 3\n")));
        SynxParseResult r = Synx.parseFull("a 1\nprototype |\n  !active\n  b:env HOME\n  ###\nc 2\n");
        assertEquals(SynxMode.STATIC, r.mode);
        assertEquals("{\"a\":1,\"c\":2}", Synx.toJson(r.root));
    }
}
