using Synx;
using Xunit;

namespace Synx.Tests;

/// <summary>
/// `- key value` followed by more deeply indented lines makes the item an object. Every expected
/// string below is the output of the Rust CLI (<c>synx parse</c>) for the same input.
/// </summary>
public class ListItemObjectTests
{
    private static string Json(string text) => SynxFormat.ToJson(SynxFormat.ParseActive(text));

    [Theory]
    // item key line with a constraint, followed by a sibling field: the constraint is checked
    [InlineData("!active\nlist\n  - port[type:int] x\n    n 1\n", "{\"list\":[{\"n\":1,\"port\":\"CONSTRAINT_ERR: 'port' expected type 'int'\"}]}")]
    // objects, scalars and a one-field object mixed in one list
    [InlineData("items\n  - name a\n    n 1\n  - name b\n    n 2\n  - scalar\n  - c 3\n    d 4\n",
        "{\"items\":[{\"n\":1,\"name\":\"a\"},{\"n\":2,\"name\":\"b\"},\"scalar\",{\"c\":3,\"d\":4}]}")]
    // nested list inside an item: it ends at the next item of the outer list
    [InlineData("items\n  - name a\n    tags\n      - x\n      - y\n    n 1\n  - name b\n",
        "{\"items\":[{\"n\":1,\"name\":\"a\",\"tags\":[\"x\",\"y\"]},\"name b\"]}")]
    // nested object inside an item
    [InlineData("items\n  - name a\n    sub\n      k 1\n      j 2\n    z 9\n  - name b\n    sub\n      k 3\n",
        "{\"items\":[{\"name\":\"a\",\"sub\":{\"j\":2,\"k\":1},\"z\":9},{\"name\":\"b\",\"sub\":{\"k\":3}}]}")]
    // type hint on the item key line; a marker on it is only ever a plain key suffix here
    [InlineData("items\n  - a(int) 5\n    b[min:1] 2\n  - x:upper y\n    q 1\nafter 1\n",
        "{\"after\":1,\"items\":[{\"a\":5,\"b\":2},{\"q\":1,\"x\":\"y\"}]}")]
    // a key without a value on the item line is an empty object
    [InlineData("!active\nitems\n  - id:random:int\n    n 1\n  - note | \n    n 2\nnext 2\n",
        "{\"items\":[{\"id\":{},\"n\":1},{\"n\":2,\"note\":\"|\"}],\"next\":2}")]
    // blank lines and trailing comments between item fields
    [InlineData("items\n  - name a\n    n 1 # c\n\n    m 2\n  - nm\n\n    p 3\n  - last\n",
        "{\"items\":[{\"m\":2,\"n\":1,\"name\":\"a\"},{\"nm\":{},\"p\":3},\"last\"]}")]
    // list under a nested key, then a sibling key and a top-level list
    [InlineData("a\n  items\n    - k v\n      z 1\n    - k w\n  other 1\nb\n  - x\n",
        "{\"a\":{\"items\":[{\"k\":\"v\",\"z\":1},\"k w\"],\"other\":1},\"b\":[\"x\"]}")]
    // list nested in an item, with fields after it
    [InlineData("items\n  - k v\n    z 1\n    sub\n      - p\n      - q\n    after 2\n  - k w\n",
        "{\"items\":[{\"after\":2,\"k\":\"v\",\"sub\":[\"p\",\"q\"],\"z\":1},\"k w\"]}")]
    // multiline block inside an item
    [InlineData("items\n  - k v\n    text |\n      line1\n      line2\n    z 1\n",
        "{\"items\":[{\"k\":\"v\",\"text\":\"line1\\nline2\",\"z\":1}]}")]
    // an item line that cannot be a key line is stored as `_value`
    [InlineData("items\n  - (int)5\n    z 1\n  - k:v w\n    z 1\n",
        "{\"items\":[{\"_value\":\"(int)5\",\"z\":1},{\"k\":\"w\",\"z\":1}]}")]
    public void List_item_objects_match_rust(string text, string expected)
    {
        Assert.Equal(expected, Json(text));
    }
}
