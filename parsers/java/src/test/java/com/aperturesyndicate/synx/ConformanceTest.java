package com.aperturesyndicate.synx;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.stream.Stream;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class ConformanceTest {

    @Test
    void corpus_parses_without_error() throws Exception {
        Path dir = findCorpus();
        if (dir == null) return; // corpus absent — skip
        int[] parsed = { 0 };
        int[] failed = { 0 };
        try (Stream<Path> files = Files.list(dir)) {
            files.filter(p -> p.toString().endsWith(".synx")).forEach(p -> {
                try {
                    String text = Files.readString(p);
                    SynxParseResult r = SynxParser.parse(text);
                    if (r.root instanceof SynxValue.Obj) parsed[0]++;
                    else failed[0]++;
                } catch (Exception e) {
                    failed[0]++;
                }
            });
        }
        System.out.println("[corpus] parsed " + parsed[0] + " files, " + failed[0] + " failed");
        assertEquals(0, failed[0]);
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
