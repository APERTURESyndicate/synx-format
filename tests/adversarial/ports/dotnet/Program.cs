// Port worker for the .NET implementation; protocol as in ports/go/main.go.
using System.Text;
using Synx;

var utf8 = new UTF8Encoding(false);
Console.InputEncoding = utf8;
Console.OutputEncoding = utf8;
string? line;
while ((line = Console.ReadLine()) != null)
{
    var f = line.Split('\t');
    if (f.Length < 5) continue;
    if (f[1] == "decompile")
    {
        // The third field is the .synxb container itself.
        string answer;
        try { answer = SynxFormat.Decompile(Convert.FromBase64String(f[2])); }
        catch (Exception e) { answer = "ERROR: " + e.GetType().Name + ": " + e.Message; }
        Console.WriteLine(f[0] + "\t" + Convert.ToBase64String(utf8.GetBytes(answer)) + "\t");
        continue;
    }
    var text = utf8.GetString(Convert.FromBase64String(f[2]));
    var env = new Dictionary<string, string>();
    foreach (var l in utf8.GetString(Convert.FromBase64String(f[3])).Split('\n'))
    {
        var i = l.IndexOf('=');
        if (i > 0) env[l[..i]] = l[(i + 1)..];
    }
    string json;
    var shown = "";
    try
    {
        var root = f[1] == "active"
            ? SynxFormat.ParseActive(text, new SynxOptions { Env = env, BasePath = f[4] })
            : SynxFormat.Parse(text);
        json = SynxFormat.ToJson(root);
        shown = string.Join(" ", root.Values.Select(v => v?.ToString() + " " + $"{v}"));
    }
    catch (Exception e)
    {
        json = "ERROR: " + e.GetType().Name + ": " + e.Message;
    }
    Console.WriteLine(f[0] + "\t" + Convert.ToBase64String(utf8.GetBytes(json)) + "\t" + Convert.ToBase64String(utf8.GetBytes(shown)));
}
