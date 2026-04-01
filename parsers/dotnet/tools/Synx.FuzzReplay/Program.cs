using System.Diagnostics;
using System.Text;
using Synx;

/// <summary>
/// Replay arbitrary files through <see cref="SynxFormat.Parse"/> (and optional JSON emit),
/// matching the Rust fuzz harness precondition: valid UTF-8 only.
/// Use with <c>cargo fuzz</c> artifact files or any corpus directory.
/// </summary>
internal static class Program
{
    private static readonly UTF8Encoding Utf8Strict = new(false, true);

    public static int Main(string[] args)
    {
        var bench = args.Any(a => a == "--bench" || a == "-b");
        var paths = args.Where(a => !a.StartsWith("-", StringComparison.Ordinal)).ToList();
        if (paths.Count == 0)
        {
            Console.Error.WriteLine("Usage: Synx.FuzzReplay [--bench] <file> [file...]");
            Console.Error.WriteLine("  Only well-formed UTF-8 inputs are parsed (same filter as fuzz_parse in Rust).");
            return args.Length == 0 ? 1 : 0;
        }

        long totalMs = 0;
        var ok = 0;
        var skipped = 0;

        foreach (var path in paths)
        {
            if (!File.Exists(path))
            {
                Console.Error.WriteLine($"MISSING: {path}");
                return 1;
            }

            byte[] bytes = File.ReadAllBytes(path);
            string text;
            try
            {
                text = Utf8Strict.GetString(bytes);
            }
            catch (DecoderFallbackException)
            {
                skipped++;
                continue;
            }

            try
            {
                var sw = Stopwatch.StartNew();
                var root = SynxFormat.Parse(text);
                _ = SynxFormat.ToJson(root);
                sw.Stop();
                if (bench)
                {
                    var ms = sw.Elapsed.TotalMilliseconds;
                    totalMs += (long)ms;
                    Console.WriteLine($"{ms:F3} ms\t{Path.GetFileName(path)}");
                }
                ok++;
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"FAIL {path}: {ex.Message}");
                return 1;
            }
        }

        if (bench && ok > 0)
            Console.WriteLine($"Total parse+tojson: {totalMs} ms over {ok} file(s); skipped non-UTF8: {skipped}");

        return 0;
    }
}
