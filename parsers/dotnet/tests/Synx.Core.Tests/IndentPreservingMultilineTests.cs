using Synx;
using Xunit;

namespace Synx.Tests;

/// <summary>
/// SYNX 3.7 §8.4.1 — the <c>|+</c> indent-preserving multiline opener.
///
/// These exist because 3.7.0 shipped without them: the Dart parser turned out
/// not to compile at all under <c>|+</c>, and nothing caught it, since only
/// Rust, JS and Go had coverage for the operator.
/// </summary>
public class IndentPreservingMultilineTests
{
    [Fact]
    public void PlusPreservesIndentRelativeToTheFirstContinuationLine()
    {
        // The worked example from the normative text.
        var text = "prompt |+\n"
                 + "  Outline:\n"
                 + "    - step one\n"
                 + "    - step two\n"
                 + "      sub-step\n"
                 + "  End.\n";

        var doc = SynxFormat.Parse(text);
        var value = doc["prompt"].AsString();

        Assert.Equal("Outline:\n  - step one\n  - step two\n    sub-step\nEnd.", value);
    }

    [Fact]
    public void PlainPipeStillTrimsEveryContinuationLine()
    {
        var text = "prompt |\n"
                 + "  Outline:\n"
                 + "    - step one\n"
                 + "  End.\n";

        var doc = SynxFormat.Parse(text);

        Assert.Equal("Outline:\n- step one\nEnd.", doc["prompt"].AsString());
    }

    [Fact]
    public void BaseIndentIsLockedOnTheFirstNonEmptyContinuationLine()
    {
        // The base is taken from the first content line (4 spaces), so the
        // shallower line that follows loses all of its indentation and no
        // padding is invented for it.
        var text = "k |+\n"
                 + "    deep\n"
                 + "  shallow\n";

        var doc = SynxFormat.Parse(text);

        Assert.Equal("deep\nshallow", doc["k"].AsString());
    }

    [Fact]
    public void TrailingWhitespaceIsStrippedPerLine()
    {
        var text = "k |+\n"
                 + "  a   \n"
                 + "  b\t\n";

        var doc = SynxFormat.Parse(text);

        Assert.Equal("a\nb", doc["k"].AsString());
    }

    [Fact]
    public void BlockEndsAtTheOpenerIndent()
    {
        var text = "k |+\n"
                 + "  body\n"
                 + "other value\n";

        var doc = SynxFormat.Parse(text);

        Assert.Equal("body", doc["k"].AsString());
        Assert.Equal("value", doc["other"].AsString());
    }

    [Fact]
    public void NonAsciiSurvivesTheSlice()
    {
        // The slice is taken at a byte offset in some implementations, so a
        // multi-byte character right after the stripped indent is the case that
        // breaks if the offset is applied to the wrong unit.
        var text = "k |+\n"
                 + "  Привет\n"
                 + "    мир\n";

        var doc = SynxFormat.Parse(text);

        Assert.Equal("Привет\n  мир", doc["k"].AsString());
    }
}
