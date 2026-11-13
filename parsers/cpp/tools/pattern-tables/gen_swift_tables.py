"""parsers/swift/Sources/Synx/SynxPatternTables.swift from the C++ port's
generated pattern_tables.inc: the same data as string literals, decoded once
(a large array literal is slow for the Swift type checker).
usage: python gen_swift_tables.py <pattern_tables.inc> <out.swift>"""
import re
import sys

src = open(sys.argv[1], encoding='utf-8').read()


def array_body(name):
    m = re.search(r'constexpr uint32_t ' + name + r'\[\] = \{(.*?)\};', src, re.S)
    return [x.strip() for x in m.group(1).replace('\n', ' ').split(',') if x.strip()]


def hexes(items):
    return ','.join(format(int(x, 0), 'x') for x in items)


set_ranges = array_body('kSetRanges')
fold = array_body('kFold')
fold_targets = array_body('kFoldTargets')
sets = re.findall(r'\{"([^"]+)", (\d+), (\d+)\}', re.search(r'constexpr NamedSet kSets\[\] = \{(.*?)\};', src, re.S).group(1))


def pairs(name):
    body = re.search(r'constexpr NamePair ' + name + r'\[\] = \{(.*?)\};', src, re.S).group(1)
    return ';'.join(f'{a}={b}' for a, b in re.findall(r'\{"([^"]*)", "([^"]*)"\}', body))


def chunked(s, n=120):
    # One multi-line literal: a long `+` chain is slow for the type checker.
    sep = ';' if ';' in s else ','
    lines, line = [], ''
    for tok in s.split(sep):
        if line and len(line) + len(tok) + 1 > n:
            lines.append(line + sep)
            line = ''
        line = tok if not line else line + sep + tok
    lines.append(line)
    body = '\n'.join('        ' + ln for ln in lines)
    return '"""\n' + body + '\n        """'


out = f'''// Unicode data for the `[pattern]` engine: the C++ port's pattern_tables.inc
// (generated from regex-syntax 0.8.10, Unicode 16.0) as text, decoded once.
// Do not edit by hand; regenerate with gen_swift_tables.py.

enum PatternTables {{
    /// Inclusive [lo, hi] pairs; `sets` names slices of them.
    static let setRanges: [UInt32] = hexList(setRangesText)
    /// name, offset (in pairs), count.
    static let sets: [String: (Int, Int)] = {{
        var out: [String: (Int, Int)] = [:]
        for raw in setsText.split(separator: ";") {{
            let item = raw.filter {{ !$0.isWhitespace }}
            let f = item.split(separator: ",")
            out[String(f[0])] = (Int(f[1])!, Int(f[2])!)
        }}
        return out
    }}()
    /// Scalar, offset in `foldTargets`, length: the simple case folding orbit.
    static let fold: [UInt32] = hexList(foldText)
    static let foldTargets: [UInt32] = hexList(foldTargetsText)
    static let propertyNames: [String: String] = pairs(propertyNamesText)
    static let generalCategories: [String: String] = pairs(generalCategoriesText)
    static let scripts: [String: String] = pairs(scriptsText)

    private static func hexList(_ s: String) -> [UInt32] {{
        s.split(whereSeparator: {{ $0 == "," || $0.isWhitespace }}).map {{ UInt32($0, radix: 16)! }}
    }}

    private static func pairs(_ s: String) -> [String: String] {{
        var out: [String: String] = [:]
        for raw in s.split(separator: ";") {{
            let item = raw.filter {{ !$0.isWhitespace }}
            let f = item.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            out[String(f[0])] = String(f[1])
        }}
        return out
    }}

    private static let setRangesText =
        {chunked(hexes(set_ranges))}
    private static let setsText =
        {chunked(';'.join(f'{n},{o},{c}' for n, o, c in sets))}
    private static let foldText =
        {chunked(hexes(fold))}
    private static let foldTargetsText =
        {chunked(hexes(fold_targets))}
    private static let propertyNamesText =
        {chunked(pairs('kPropertyNames'))}
    private static let generalCategoriesText =
        {chunked(pairs('kGeneralCategories'))}
    private static let scriptsText =
        {chunked(pairs('kScripts'))}
}}
'''
open(sys.argv[2], 'w', encoding='utf-8', newline='\n').write(out)
print(sys.argv[2], len(out), 'bytes')
