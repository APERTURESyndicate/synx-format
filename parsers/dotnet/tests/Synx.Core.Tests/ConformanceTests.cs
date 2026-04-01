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
        var input = File.ReadAllText(synxPath);
        var expected = File.ReadAllText(expectedPath).Trim();

        var isTool = input.TrimStart().StartsWith("!tool", StringComparison.Ordinal);
        string json;
        if (isTool)
        {
            var map = SynxFormat.ParseTool(input);
            json = SynxFormat.ToJson(map);
        }
        else
        {
            var map = SynxFormat.Parse(input);
            json = SynxFormat.ToJson(map);
        }

        Assert.Equal(expected, json);
    }
}
