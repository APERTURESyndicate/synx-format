// SYNX parse result — root tree plus directives and metadata. Parity with Rust `ParseResult`.
#pragma once

#include <vector>

#include "synx/meta.hpp"
#include "synx/options.hpp"
#include "synx/value.hpp"

namespace synx {

struct ParseResult {
    Value root = Value::make_object();
    Mode mode = Mode::Static;
    bool locked = false;
    /// `!tool` directive — file is an LLM tool envelope.
    bool tool = false;
    /// `!schema` directive — tool schema (paired with `!tool`).
    bool schema = false;
    /// `!llm` directive — LLM-oriented envelope (semantic hints only).
    bool llm = false;
    /// Metadata for each nesting level, keyed by dot-path prefix.
    /// `""` = root level, `"server"` = server sub-object, etc.
    MetadataTree metadata;
    /// `!include` directives parsed from the file.
    std::vector<IncludeDirective> includes;
    /// `!use` directives parsed from the file (package imports).
    std::vector<UseDirective> uses;
};

} // namespace synx
