using System.Text;
using Synx;
using Xunit;

namespace Synx.Tests;

public class ConformanceTests
{
    private static string RepoRoot()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir != null)
        {
            if (File.Exists(Path.Combine(dir.FullName, "Cargo.toml"))
                && Directory.Exists(Path.Combine(dir.FullName, "tests", "conformance", "cases")))
                return dir.FullName;
            dir = dir.Parent;
        }
        throw new InvalidOperationException("synx-format repo root not found (Cargo.toml + tests/conformance/cases).");
    }

    public static IEnumerable<object[]> Cases()
    {
        var cases = Path.Combine(RepoRoot(), "tests", "conformance", "cases");
        foreach (var synx in Directory.GetFiles(cases, "*.synx").OrderBy(Path.GetFileName))
        {
            var expectedPath = Path.ChangeExtension(synx, "expected.json");
            if (File.Exists(expectedPath))
                yield return new object[] { synx, expectedPath };
        }
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public void Conformance_case_matches_rust_json(string synxPath, string expectedPath)
    {
        // GetString keeps a leading BOM (ReadAllText would drop it): stripping it is the parser's job.
        var utf8 = new UTF8Encoding(false);
        var input = utf8.GetString(File.ReadAllBytes(synxPath));
        // As the Rust and TypeScript runners: the expectation is trimmed, our output is not.
        var expected = utf8.GetString(File.ReadAllBytes(expectedPath)).Trim();

        var isTool = input.TrimStart().StartsWith("!tool", StringComparison.Ordinal);
        string json;
        if (isTool)
        {
            var map = SynxFormat.ParseTool(input);
            json = SynxFormat.ToJson(map);
        }
        else
        {
            // Active documents (also behind a BOM) are resolved with default options; static ones are not.
            var parsed = SynxFormat.ParseFullActive(input);
            json = SynxFormat.ToJson(parsed.Root);
        }

        Assert.True(expected == json,
            $"{Path.GetFileName(synxPath)}{Environment.NewLine}  got:      {json}{Environment.NewLine}  expected: {expected}");
    }

    [Fact]
    public void Conformance_suite_is_not_empty()
    {
        Assert.True(Cases().Count() >= 34, "expected at least 34 conformance cases (001-019, 100-114)");
    }
}
