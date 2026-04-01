# Demo: run from `bindings/mojo` after installing `synx-format` for the active Python.
#   mojo run examples/demo.mojo

from synx.interop import parse_json


def main() raises:
    var j = parse_json("name Demo\ncount 3\n")
    print(j)
