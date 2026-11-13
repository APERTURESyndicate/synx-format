import com.sun.jna.Library;
import com.sun.jna.Native;
import com.sun.jna.NativeLong;
import com.sun.jna.Pointer;
import com.sun.jna.ptr.LongByReference;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;

/**
 * The C library reached the way the Kotlin binding reaches it: the same JNA
 * mapping (String for `const char*`, NativeLong for `size_t`, UTF-8 strings).
 *
 *   java -cp jna.jar Main.java <directory with synx_c.dll>
 */
public class Main {
    public interface CSynx extends Library {
        Pointer synx_parse(String input);
        Pointer synx_parse_active_ex(String input, String envJson, String basePath);
        Pointer synx_format(String input);
        Pointer synx_stringify(String json);
        Pointer synx_diff(String a, String b);
        void synx_free(Pointer p);
        Pointer synx_compile(String input, int resolved, LongByReference outLen);
        void synx_free_bytes(Pointer p, NativeLong len);
        Pointer synx_decompile(byte[] data, NativeLong len);
        int synx_is_synxb(byte[] data, NativeLong len);
    }

    static CSynx lib;
    static final char NUL = (char) 0;
    static final char LONE = (char) 0xd800;

    static String take(Pointer p) {
        if (p == null) return "<NULL>";
        try { return p.getString(0, "UTF-8"); } finally { lib.synx_free(p); }
    }

    static String show(String s) {
        StringBuilder b = new StringBuilder();
        for (char c : s.toCharArray()) {
            if (c == '\n') b.append("<LF>");
            else if (c < 32 || c > 126) b.append("<").append(Integer.toHexString(c)).append(">");
            else b.append(c);
        }
        return b.toString();
    }

    public static void main(String[] args) {
        System.setProperty("jna.library.path", args[0]);
        Map<String, Object> opts = new HashMap<>();
        opts.put(Library.OPTION_STRING_ENCODING, StandardCharsets.UTF_8.name());
        lib = Native.load("synx_c", CSynx.class, opts);

        String doc = "name x\nrole guest" + NUL + "\nrole admin\nport 1\n";
        System.out.println("parse      : " + show(take(lib.synx_parse(doc))));
        System.out.println("format     : " + show(take(lib.synx_format(doc))));
        System.out.println("active     : " + show(take(lib.synx_parse_active_ex("!active\nlimit 5" + NUL + "\nlimit[max:3] 9\n", "{}", null))));
        System.out.println("diff       : " + show(take(lib.synx_diff("a 1\nb 2\n", "a 1" + NUL + "\nb 3\n"))));
        LongByReference n = new LongByReference();
        Pointer bin = lib.synx_compile(doc, 0, n);
        byte[] bytes = bin.getByteArray(0, (int) n.getValue());
        lib.synx_free_bytes(bin, new NativeLong(n.getValue()));
        System.out.println("compile    : " + bytes.length + " bytes; decompile: " + show(take(lib.synx_decompile(bytes, new NativeLong(bytes.length)))));
        System.out.println("stringify  : " + show(take(lib.synx_stringify("{\"a\":\"x" + "\\" + "u0000y\",\"b\":2}"))));
        System.out.println("env        : " + show(take(lib.synx_parse_active_ex("!active\nk:env:default:none K\n", "{\"K\":\"v" + "\\" + "u0000hidden\"}", null))));
        System.out.println("lone       : " + show(take(lib.synx_parse("a " + LONE + "x\nb 2\n"))));
        System.out.println("NativeLong : " + NativeLong.SIZE + " bytes, size_t: " + Native.SIZE_T_SIZE + " bytes");
        System.out.println("is_synxb   : " + lib.synx_is_synxb(bytes, new NativeLong(bytes.length)));
    }
}
