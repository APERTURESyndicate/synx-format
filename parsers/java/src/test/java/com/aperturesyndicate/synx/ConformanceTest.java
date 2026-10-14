package com.aperturesyndicate.synx;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;
import java.util.stream.Stream;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/** Byte-exact check of every {@code tests/conformance/cases/*.synx} against its {@code .expected.json} golden. */
class ConformanceTest {

    @Test
    void corpus_matches_goldens_byte_for_byte() throws IOException {
        Path dir = findCorpus();
        assertNotNull(dir, "tests/conformance/cases not found from " + Paths.get("").toAbsolutePath());

        List<Path> inputs;
        try (Stream<Path> files = Files.list(dir)) {
            inputs = files.filter(p -> p.toString().endsWith(".synx")).sorted().toList();
        }

        int checked = 0;
        List<String> mismatches = new ArrayList<>();
        for (Path input : inputs) {
            String name = input.getFileName().toString();
            Path golden = dir.resolve(name.substring(0, name.length() - ".synx".length()) + ".expected.json");
            if (!Files.exists(golden)) continue;
            checked++;

            // readString keeps a leading BOM: stripping it is the parser's job.
            String text = Files.readString(input, StandardCharsets.UTF_8);
            String expected = Files.readString(golden, StandardCharsets.UTF_8).strip();

            String got;
            try {
                if (text.stripLeading().startsWith("!tool")) {
                    got = Synx.toJson(Synx.parseTool(text));
                } else {
                    // Active documents (also behind a BOM) are resolved with default options.
                    got = Synx.toJson(Synx.parseFullActive(text, new SynxOptions()).root);
                }
            } catch (RuntimeException e) {
                got = "<threw " + e + ">";
            }
            if (!got.equals(expected)) {
                mismatches.add(name + "\n  got:      " + got + "\n  expected: " + expected);
            }
        }

        assertTrue(checked >= 24, "expected at least 24 conformance cases, found " + checked);
        assertTrue(mismatches.isEmpty(), mismatches.size() + " of " + checked
            + " conformance cases differ:\n" + String.join("\n", mismatches));
    }

    private static Path findCorpus() {
        String[] candidates = {
            "tests/conformance/cases",
            "../tests/conformance/cases",
            "../../tests/conformance/cases",
            "../../../tests/conformance/cases",
            "../../../../tests/conformance/cases",
        };
        Path cwd = Paths.get("").toAbsolutePath();
        for (String c : candidates) {
            Path abs = cwd.resolve(c).normalize();
            if (Files.isDirectory(abs)) return abs;
        }
        return null;
    }
}
