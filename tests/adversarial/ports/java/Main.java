// Port worker for the Java implementation; protocol as in ports/go/main.go.
import com.aperturesyndicate.synx.Synx;
import com.aperturesyndicate.synx.SynxObject;
import com.aperturesyndicate.synx.SynxOptions;

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.HashMap;
import java.util.Map;

public final class Main {
    public static void main(String[] args) throws Exception {
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        PrintStream out = new PrintStream(System.out, false, "UTF-8");
        Base64.Decoder dec = Base64.getDecoder();
        Base64.Encoder enc = Base64.getEncoder();
        String line;
        while ((line = in.readLine()) != null) {
            String[] f = line.split("\t", -1);
            if (f.length < 5) continue;
            if (f[1].equals("decompile")) {
                // The third field is the .synxb container itself.
                String answer;
                try {
                    com.aperturesyndicate.synx.SynxBinary.Outcome<String> r = Synx.decompile(dec.decode(f[2]));
                    answer = r.ok ? r.value : "ERROR: " + r.error;
                } catch (Throwable e) {
                    answer = "ERROR: " + e.getClass().getSimpleName() + ": " + e.getMessage();
                }
                out.println(f[0] + "\t" + enc.encodeToString(answer.getBytes(StandardCharsets.UTF_8)) + "\t");
                continue;
            }
            String text = new String(dec.decode(f[2]), StandardCharsets.UTF_8);
            Map<String, String> env = new HashMap<>();
            for (String l : new String(dec.decode(f[3]), StandardCharsets.UTF_8).split("\n")) {
                int i = l.indexOf('=');
                if (i > 0) env.put(l.substring(0, i), l.substring(i + 1));
            }
            String json;
            String shown = "";
            try {
                SynxObject root;
                if (f[1].equals("active")) {
                    SynxOptions o = new SynxOptions();
                    o.env = env;
                    o.basePath = f[4];
                    root = Synx.parseActive(text, o);
                } else {
                    root = Synx.parse(text);
                }
                json = Synx.toJson(root);
                shown = String.valueOf(root) + " " + String.format("%s", root);
            } catch (Throwable e) {
                json = "ERROR: " + e.getClass().getSimpleName() + ": " + e.getMessage();
            }
            out.println(f[0] + "\t" + enc.encodeToString(json.getBytes(StandardCharsets.UTF_8)) + "\t" + enc.encodeToString(shown.getBytes(StandardCharsets.UTF_8)));
        }
        out.flush();
    }
}
