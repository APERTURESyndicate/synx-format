using System.Diagnostics;
using System.IO.Compression;
using System.Text;
using System.Text.Json;
using Synx;
using Xunit;

namespace Synx.Tests;

/// <summary>
/// Inputs that crashed the process, hung it, printed a secret or disagreed with synx-core.
/// Every expected value is synx-core's output for the same input.
/// </summary>
public class HardeningTests
{
    private static SynxOptions NoEnv(string? dir = null) => new() { Env = new Dictionary<string, string>(), BasePath = dir };

    private static string Active(string body, string? dir = null) =>
        SynxFormat.ToJson(SynxFormat.ParseActive("!active\n" + body, NoEnv(dir)));

    private static string TempDir(string tag) => Directory.CreateTempSubdirectory("synx-" + tag + "-").FullName;

    /// <summary>A directory link that needs no privilege: a junction on Windows, a symlink elsewhere.</summary>
    private static bool TryLinkDirectory(string link, string target)
    {
        try
        {
            if (OperatingSystem.IsWindows())
            {
                using var p = Process.Start(new ProcessStartInfo("cmd.exe", $"/C mklink /J \"{link}\" \"{target}\"")
                {
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                });
                p!.WaitForExit();
                return p.ExitCode == 0 && Directory.Exists(link);
            }
            Directory.CreateSymbolicLink(link, target);
            return true;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        {
            return false;
        }
    }

    // ── Secrets ──

    [Fact]
    public void A_secret_is_printed_nowhere()
    {
        var map = SynxFormat.ParseActive("!active\npassword:secret hunter2\nport 8080\nname Alice\n");
        var secret = map["password"];
        Assert.IsType<SynxValue.Secret>(secret);
        Assert.Equal("[SECRET]", secret.ToString());
        Assert.Equal("[SECRET]", $"{secret}");
        Assert.DoesNotContain("hunter2", string.Join(",", map.Values));
        Assert.DoesNotContain("hunter2", JsonSerializer.Serialize<object>(secret));
        Assert.DoesNotContain("hunter2", JsonSerializer.Serialize(new { cfg = (object)secret, all = map.Values.Cast<object>().ToArray() }));
        Assert.DoesNotContain("hunter2", SynxFormat.ToJson(map));
        Assert.DoesNotContain("hunter2", SynxFormat.Stringify(map));

        // The typed accessors are the one way to the plaintext.
        Assert.Equal("hunter2", secret.AsString());
        Assert.Equal("hunter2", ((SynxValue.Secret)secret).Value);

        // The debugger shows the mask and does not list the member.
        var display = (DebuggerDisplayAttribute)Assert.Single(typeof(SynxValue.Secret).GetCustomAttributes(typeof(DebuggerDisplayAttribute), false));
        Assert.Equal("[SECRET]", display.Value);
        var browsable = (DebuggerBrowsableAttribute)Assert.Single(
            typeof(SynxValue.Secret).GetProperty("Value")!.GetCustomAttributes(typeof(DebuggerBrowsableAttribute), false));
        Assert.Equal(DebuggerBrowsableState.Never, browsable.State);

        // The other values print as the README says.
        Assert.Equal("8080", map["port"].ToString());
        Assert.Equal("Alice", $"{map["name"]}");
        Assert.Equal("true", new SynxValue.Bool(true).ToString());
        Assert.Equal("null", new SynxValue.Null().ToString());
    }

    [Fact]
    public void An_unset_secret_is_null_and_required_notices_it()
    {
        var map = SynxFormat.ParseActive(
            "!active\ns:secret hunter2\nu:secret:env SYNX_UNSET_Q\nr[required]:secret:env SYNX_UNSET_Q\ng[required]\n", NoEnv());
        Assert.Equal("{\"g\":\"CONSTRAINT_ERR: 'g' is required\",\"r\":\"CONSTRAINT_ERR: 'r' is required\",\"s\":\"[SECRET]\",\"u\":null}",
            SynxFormat.ToJson(map));
        Assert.IsType<SynxValue.Null>(map["u"]);
    }

    // ── :calc ──

    [Fact]
    public void Calc_parentheses_never_use_the_call_stack()
    {
        // A stack overflow cannot be caught: before, this document ended the process.
        string? json = null;
        var thread = new Thread(() =>
        {
            json = Active("x:calc " + new string('(', 1500) + "7" + new string(')', 1500) + "\n"
                          + "y:calc " + new string('(', 4000) + "1\nbase 5\n"
                          + "z:ref:calc:+" + new string('(', 20000) + "1" + new string(')', 20000) + " base\n");
        }, 256 * 1024);
        thread.Start();
        thread.Join();
        Assert.Equal("{\"base\":5,\"x\":7,\"y\":\"CALC_ERR: SYNX :calc — missing closing parenthesis\",\"z\":6}", json);
    }

    [Fact]
    public void Calc_without_recursion_gives_the_same_answers()
    {
        Assert.Equal("{\"a\":9,\"b\":\"CALC_ERR: SYNX :calc — invalid number: '-'\",\"c\":\"CALC_ERR: SYNX :calc — missing closing parenthesis\","
                     + "\"d\":12,\"e\":\"CALC_ERR: SYNX :calc — division by zero\",\"f\":\"CALC_ERR: SYNX :calc — unexpected token: ')'\",\"g\":1,"
                     + "\"h\":\"CALC_ERR: SYNX :calc — missing closing parenthesis\",\"i\":\"CALC_ERR: SYNX :calc — unexpected token at position 3\"}",
            Active("a:calc ((1+2)*3)\nb:calc -(2)\nc:calc (1 + 2\nd:calc 2 * (3 + 4) - 5 % 3\ne:calc 1 / (2 - 2)\nf:calc ()\n"
                   + "g:calc ((((((((((1))))))))))\nh:calc 2 * (3\ni:calc (2))\n"));
    }

    // ── Files ──

    [Fact]
    public void An_include_cycle_stops_at_its_first_repeat()
    {
        var dir = TempDir("cycle");
        try
        {
            // Three keys including the file they are in: 3^16 resolutions under the depth cap alone.
            File.WriteAllText(Path.Combine(dir, "self.synx"), "!active\na:include self.synx\nb:include self.synx\nc:include self.synx\nv 1\n");
            File.WriteAllText(Path.Combine(dir, "ping.synx"), "!active\nnext:include pong.synx\np 1\n");
            File.WriteAllText(Path.Combine(dir, "pong.synx"), "!active\nnext:include ping.synx\nq 2\n");
            var sw = Stopwatch.StartNew();
            var json = Active("a:include self.synx\nb:include self.synx\nc:include self.synx\n", dir);
            sw.Stop();
            const string level = "{\"a\":\"INCLUDE_ERR: circular include: 'self.synx'\",\"b\":\"INCLUDE_ERR: circular include: 'self.synx'\","
                                 + "\"c\":\"INCLUDE_ERR: circular include: 'self.synx'\",\"v\":1}";
            Assert.Equal("{\"a\":" + level + ",\"b\":" + level + ",\"c\":" + level + "}", json);
            Assert.True(sw.ElapsedMilliseconds < 3000, $"took {sw.ElapsedMilliseconds} ms");

            Assert.Equal("{\"x\":{\"next\":{\"next\":\"INCLUDE_ERR: circular include: 'ping.synx'\",\"q\":2},\"p\":1}}",
                Active("x:include ping.synx\n", dir));
            // The same file from two keys is no cycle, and a later document starts afresh.
            Assert.Equal("{\"x\":{\"next\":{\"next\":\"INCLUDE_ERR: circular include: 'ping.synx'\",\"q\":2},\"p\":1}}",
                Active("x:include ping.synx\n", dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void One_document_reads_at_most_1024_included_files()
    {
        var dir = TempDir("budget");
        try
        {
            var body = new StringBuilder();
            for (var i = 0; i < 1100; i++)
            {
                File.WriteAllText(Path.Combine(dir, $"inc{i}.synx"), $"v {i}\n");
                body.Append($"k{i:D4}:include inc{i}.synx\n");
            }
            var map = SynxFormat.ParseActive("!active\n" + body, NoEnv(dir));
            Assert.Equal(new SynxValue.Int(1023), map["k1023"]["v"]);
            Assert.Equal(new SynxValue.Str("INCLUDE_ERR: include budget (1024 files) exceeded"), map["k1024"]);
            Assert.Equal(76, map.Values.Count(v => v is SynxValue.Str));
            // The budget belongs to one document.
            Assert.Equal("{\"k\":{\"v\":0}}", Active("k:include inc0.synx\n", dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void Packages_that_use_each_other_do_not_recurse_forever()
    {
        var dir = TempDir("use");
        try
        {
            var packages = Path.Combine(dir, "synx_packages");
            Directory.CreateDirectory(Path.Combine(packages, "@a", "one", "src"));
            Directory.CreateDirectory(Path.Combine(packages, "@a", "two", "src"));
            File.WriteAllText(Path.Combine(packages, "@a", "one", "src", "main.synx"), "!active\n!use @a/two\nv 1\n");
            File.WriteAllText(Path.Combine(packages, "@a", "two", "src", "main.synx"), "!active\n!use @a/one\nw 2\n");
            var options = NoEnv(dir);
            options.PackagesPath = packages;
            string? json = null;
            var thread = new Thread(() => json = SynxFormat.ToJson(SynxFormat.ParseActive("!active\n!use @a/one\nx 1\n", options)), 256 * 1024);
            thread.Start();
            thread.Join();
            Assert.Equal("{\"one\":{\"two\":{\"w\":2},\"v\":1},\"x\":1}", json);
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void A_link_inside_the_base_does_not_lead_out_of_it()
    {
        var root = TempDir("jail");
        try
        {
            var baseDir = Path.Combine(root, "base");
            var outside = Path.Combine(root, "outside");
            Directory.CreateDirectory(baseDir);
            Directory.CreateDirectory(outside);
            Directory.CreateDirectory(Path.Combine(baseDir, "sub"));
            File.WriteAllText(Path.Combine(outside, "secret.synx"), "token OUTSIDE\n");
            File.WriteAllText(Path.Combine(baseDir, "sub", "part.synx"), "v 1\n");
            if (!TryLinkDirectory(Path.Combine(baseDir, "link"), outside)) return;

            var json = Active("a:include link/secret.synx\nb:watch link/secret.synx\nc:fallback:none link/secret.synx\nd:include sub/part.synx\n", baseDir);
            Assert.DoesNotContain("OUTSIDE", json);
            Assert.Equal("{\"a\":\"INCLUDE_ERR: SECURITY: path escapes base directory: 'link/secret.synx'\","
                         + "\"b\":\"WATCH_ERR: SECURITY: path escapes base directory: 'link/secret.synx'\",\"c\":\"none\",\"d\":{\"v\":1}}", json);
        }
        finally
        {
            // The junction is removed first: deleting through it would empty its target.
            var link = Path.Combine(root, "base", "link");
            if (Directory.Exists(link)) Directory.Delete(link, false);
            Directory.Delete(root, true);
        }
    }

    [Fact]
    public void File_errors_carry_no_host_path_and_read_the_same_everywhere()
    {
        var dir = TempDir("paths");
        try
        {
            Directory.CreateDirectory(Path.Combine(dir, "sub"));
            File.WriteAllText(Path.Combine(dir, "sub", "part.synx"), "v 1\n");
            File.WriteAllBytes(Path.Combine(dir, "bad.synx"), [0x76, 0x20, 0xff, 0xfe, 0x0a]);
            var longName = new string('x', 300) + ".synx";
            var json = Active("a:include nope.synx\nd:include sub\ne:include .\nf:include " + longName + "\ng:include sub/../sub/part.synx\n"
                              + "h:include sub/../nope.synx\nk:watch sub\nl:watch nope.json\nu:include bad.synx\n", dir);
            Assert.DoesNotContain(Path.GetFileName(dir), json);
            Assert.Equal("{\"a\":\"INCLUDE_ERR: file not found: nope.synx\",\"d\":\"INCLUDE_ERR: not a file: sub\",\"e\":\"INCLUDE_ERR: not a file: .\","
                         + "\"f\":\"INCLUDE_ERR: file not found: " + longName + "\",\"g\":{\"v\":1},"
                         + "\"h\":\"INCLUDE_ERR: SECURITY: path traversal detected: 'sub/../nope.synx'\",\"k\":\"WATCH_ERR: not a file: sub\","
                         + "\"l\":\"WATCH_ERR: file not found: nope.json\",\"u\":\"INCLUDE_ERR: file is not valid UTF-8: bad.synx\"}", json);

            if (OperatingSystem.IsWindows())
            {
                // A device name opens a device in any directory; it is a file that is not there.
                Assert.Equal("{\"i\":\"INCLUDE_ERR: file not found: NUL\",\"j\":\"INCLUDE_ERR: file not found: con.txt\",\"m\":\"WATCH_ERR: file not found: sub/COM1\"}",
                    Active("i:include NUL\nj:include con.txt\nm:watch sub/COM1\n", dir));
            }

            // A base directory that does not exist jails nothing, so nothing is read through it.
            Assert.Equal("{\"x\":\"INCLUDE_ERR: SECURITY: base directory not found: '" + Path.Combine(dir, "gone").Replace("\\", "\\\\") + "'\"}",
                Active("x:include part.synx\n", Path.Combine(dir, "gone")));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    // ── [pattern] ──

    [Fact]
    public void A_pattern_is_checked_in_linear_time_and_never_guessed()
    {
        var sw = Stopwatch.StartNew();
        var map = SynxFormat.ParseActive(
            "!active\nk[pattern:(.*)(.*)(.*)(.*)(.*)(.*)(.*)x] " + new string('a', 300) + "\n"
            + "v[pattern:^(a|a)*$] " + new string('a', 34) + "b\n"
            + "w[pattern:^(a|b)*$] " + string.Concat(Enumerable.Repeat("ab", 5000)) + "\n"
            + "n[pattern:^(a+)+$] " + new string('a', 40) + "b\nok[pattern:^[a-z]+$] abc\nbad[pattern:^[a-z]+$] ABC\n", NoEnv());
        sw.Stop();
        // The backtracking engine gave up after a second and let the value through.
        Assert.Equal(new SynxValue.Str("CONSTRAINT_ERR: 'k' does not match pattern /(.*)(.*)(.*)(.*)(.*)(.*)(.*)x/"), map["k"]);
        Assert.Equal(new SynxValue.Str("CONSTRAINT_ERR: 'v' does not match pattern /^(a|a)*$/"), map["v"]);
        Assert.Equal(10000, map["w"].AsString()!.Length);
        // A nested quantifier is not checked at all, as in synx-core.
        Assert.Equal(new string('a', 40) + "b", map["n"].AsString());
        Assert.Equal(new SynxValue.Str("abc"), map["ok"]);
        Assert.Equal(new SynxValue.Str("CONSTRAINT_ERR: 'bad' does not match pattern /^[a-z]+$/"), map["bad"]);
        Assert.True(sw.ElapsedMilliseconds < 2000, $"took {sw.ElapsedMilliseconds} ms");
    }

    // ── Numbers ──

    [Fact]
    public void Round_sort_sum_and_outside_numbers_follow_synx_core()
    {
        Assert.Equal("{\"big\":9.223372036854776e18,\"q\":\"abc\",\"r\":3,\"s\":-3,\"t\":1.0,\"u\":\"1e18\",\"v\":5,\"w\":\"1e30\",\"x\":null,\"y\":null,\"z\":2}",
            Active("x(float):round nan\ny(float):round inf\nz:round:99999999999 1.5\nv:round:-1 5\nw:round 1e30\nq:round abc\nr:round 2.5\ns:round -2.5\n"
                   + "t:round:2 1.005\nu:round 1e18\nbig:round 9223372036854775807\n"));

        Assert.Equal("{\"big\":\"1e400\",\"d\":[\"b\",\"1a\",10,9,2.5],\"g\":5,\"h\":5,\"host\":\"inf\",\"i\":5,\"j\":5,"
                     + "\"k\":\"CONSTRAINT_ERR: 'k' value 2 is below min 3\",\"n\":\"NaN\",\"ok\":1000.0,\"parts\":[\"inf\",\"nan\",1,2.5],"
                     + "\"s\":[2.5,9,10,\"1a\",\"B\",\"a\",\"b\",null,true],\"sum\":3.5}",
            Active("s:sort\n  - b\n  - 10\n  - 9\n  - 1a\n  - a\n  - 2.5\n  - true\n  - null\n  - B\nd:sort:desc\n  - b\n  - 10\n  - 9\n  - 1a\n  - 2.5\n"
                   + "sum:sum\n  - 1\n  - inf\n  - nan\n  - 2.5\n  - 1e400\nhost:env:default:inf SYNX_PROBE_UNSET\nn:env:default:NaN SYNX_PROBE_UNSET\n"
                   + "big:env:default:1e400 SYNX_PROBE_UNSET\nok:env:default:1e3 SYNX_PROBE_UNSET\nparts:split inf,nan,1,2.5\n"
                   + "g[min:1e400] 5\nh[max:-inf] 5\ni[min:inf] 5\nj[max:nan] 5\nk[min:3] 2\n"));

        // A weight is digits[.digits]; anything else is the value.
        Assert.Equal("{\"a\":\"inf\",\"b\":-5,\"c\":\"1e2\",\"d\":\".5\",\"e\":\"+3\",\"f\":\"nan\"}",
            Active("a:random inf\nb:random -5\nc:random 1e2\nd:random .5\ne:random +3\nf:random nan\n"));
    }

    [Fact]
    public void A_spam_window_of_any_size_is_an_answer_not_an_exception()
    {
        Assert.Equal("{\"k2\":\"y\",\"key\":\"x\"}", Active("key:spam:1:18446744073709551615 x\nk2:spam:5:1 y\n"));
    }

    [Fact]
    public void Random_values_survive_parallel_parsing()
    {
        Parallel.For(0, 8, _ =>
        {
            for (var i = 0; i < 20000; i++) SynxFormat.Parse("v(random) 0\nf(random:float) 0\nb(random:bool) 0\n");
        });
        var seen = new HashSet<long>();
        for (var i = 0; i < 64; i++) seen.Add(SynxFormat.Parse("v(random) 0\n")["v"].AsInt()!.Value);
        // A seeded Random shared by threads breaks for good and repeats one value.
        Assert.True(seen.Count > 32, $"{seen.Count} distinct values of 64");
    }

    // ── .synxb ──

    private const byte TagNull = 0, TagString = 5, TagArray = 6, TagObject = 7;

    private static byte[] Varint(ulong n)
    {
        var bytes = new List<byte>();
        while (true)
        {
            var b = (byte)(n & 0x7f);
            n >>= 7;
            if (n != 0) bytes.Add((byte)(b | 0x80));
            else
            {
                bytes.Add(b);
                return bytes.ToArray();
            }
        }
    }

    private static byte[] Join(params byte[][] parts) => parts.SelectMany(p => p).ToArray();

    private static byte[] Table(params string[] strings)
    {
        var parts = new List<byte[]> { Varint((ulong)strings.Length) };
        foreach (var s in strings)
        {
            var utf8 = Encoding.UTF8.GetBytes(s);
            parts.Add(Varint((ulong)utf8.Length));
            parts.Add(utf8);
        }
        return Join(parts.ToArray());
    }

    private static byte[] Container(byte[] payload, uint? declared = null)
    {
        using var packed = new MemoryStream();
        using (var deflate = new DeflateStream(packed, CompressionLevel.Optimal, leaveOpen: true))
            deflate.Write(payload);
        return Join("SYNXB"u8.ToArray(), [1, 0], BitConverter.GetBytes(declared ?? (uint)payload.Length), packed.ToArray());
    }

    private static string Refusal(byte[] file) => Assert.Throws<InvalidDataException>(() => SynxFormat.Decompile(file)).Message;

    [Fact]
    public void Synxb_header_alone_reserves_nothing()
    {
        // 13 bytes declaring 256 MiB: the buffer used to be allocated before a byte was inflated.
        var file = Join("SYNXB"u8.ToArray(), [1, 0], BitConverter.GetBytes(0x10000000u), [0x03, 0x00]);
        var before = GC.GetAllocatedBytesForCurrentThread();
        var message = Refusal(file);
        var allocated = GC.GetAllocatedBytesForCurrentThread() - before;
        Assert.Equal("size mismatch: expected 268435456, got 0", message);
        Assert.True(allocated < 4 * 1024 * 1024, $"allocated {allocated} bytes");
    }

    [Fact]
    public void Synxb_bombs_are_refused_by_the_decode_budget()
    {
        const string refusal = "decoded document exceeds 268435456 bytes — refusing to decode";
        // Eight million nulls: 8 KiB of file.
        var nulls = new byte[8 * 1024 * 1024];
        Assert.Equal(refusal, Refusal(Container(Join(Table(), [TagArray], Varint((ulong)nulls.Length), nulls))));

        // One 120 KB string referenced 6000 times: 720 MB of text from 200 bytes of file.
        var refs = new List<byte>();
        for (var i = 0; i < 6000; i++)
        {
            refs.Add(TagString);
            refs.Add(0);
        }
        Assert.Equal(refusal, Refusal(Container(Join(Table(new string('x', 120000)), [TagArray], Varint(6000), refs.ToArray()))));

        // A document within the budget still reads.
        Assert.Equal("k v\n", SynxFormat.Decompile(Container(Join(Table("k", "v"), [TagObject], Varint(1), Varint(0), [TagString], Varint(1)))));
    }

    [Fact]
    public void Synxb_lengths_and_indexes_are_checked_without_overflow()
    {
        Assert.Equal("unexpected end of data in string table", Refusal(Container(Join(Varint(1), Varint(ulong.MaxValue), [1]))));
        Assert.Equal("unexpected end of data in string table", Refusal(Container(Join(Varint(1), Varint(ulong.MaxValue - 5), "abcdefgh"u8.ToArray()))));
        Assert.Equal("string index out of range",
            Refusal(Container(Join(Table("zero", "k"), [TagObject], Varint(1), Varint(1), [TagString], Varint(1UL << 32)))));
        Assert.Equal("unexpected end of data", Refusal(Container(Join(Table(), [TagArray], Varint((1UL << 63) + 5)))));
        Assert.Equal("size mismatch: expected 1024, got 1025", Refusal(Container(new byte[8 * 1024 * 1024], declared: 1024)));
    }

    // B-076: the lines under a rejected key went to whatever was open above it.
    [Fact]
    public void The_block_of_a_rejected_key_is_dropped()
    {
        Assert.Equal("{\"user\":{\"age\":3,\"profile\":{\"name\":\"x\"}}}",
            SynxFormat.ToJson(SynxFormat.Parse("user\n  profile\n    name x\n  __proto__\n    role admin\n    name y\n  age 3\n")));
        Assert.Equal("{\"limits\":{\"max\":10},\"use\":20}", Active("limits\n  max 10\n  constructor\n    max 99\nuse:calc limits.max * 2\n"));
        Assert.Equal("{\"a\":1,\"b\":2}", SynxFormat.ToJson(SynxFormat.Parse("a 1\nprototype |\n  !active\n  x:env PATH\nb 2\n")));
    }
}
