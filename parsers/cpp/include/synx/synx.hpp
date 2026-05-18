// SYNX — public top-level facade. Mirrors Rust `Synx` struct in synx-core/src/lib.rs.
#pragma once

#include <string>
#include <string_view>
#include <vector>

#include "synx/binary.hpp"
#include "synx/diff.hpp"
#include "synx/engine.hpp"
#include "synx/formatter.hpp"
#include "synx/json.hpp"
#include "synx/options.hpp"
#include "synx/parse_result.hpp"
#include "synx/parser.hpp"
#include "synx/result.hpp"
#include "synx/stringify.hpp"
#include "synx/value.hpp"

namespace synx {

class Synx {
public:
    /// Parse SYNX text and return its top-level Object (static mode only).
    /// If text declares `!active`, markers are *not* resolved — use `parse_active`.
    static Object parse_root(std::string_view text) {
        ParseResult r = ::synx::parse(text);
        if (Object* obj = r.root.as_object_mut()) return std::move(*obj);
        return {};
    }

    /// Parse and run `!active` engine resolution.
    static Object parse_active_root(std::string_view text, const Options& opts = {}) {
        ParseResult r = ::synx::parse(text);
        if (r.mode == Mode::Active) {
            ::synx::resolve(r, opts);
        }
        if (Object* obj = r.root.as_object_mut()) return std::move(*obj);
        return {};
    }

    /// Parse and return the full ParseResult (mode, metadata, includes, …).
    static ParseResult parse_full(std::string_view text) {
        return ::synx::parse(text);
    }

    /// Parse and resolve, return the full ParseResult.
    static ParseResult parse_full_active(std::string_view text, const Options& opts = {}) {
        ParseResult r = ::synx::parse(text);
        if (r.mode == Mode::Active) {
            ::synx::resolve(r, opts);
        }
        return r;
    }

    /// Parse a `!tool` envelope into `{ tool, params }` or `{ tools: [...] }`
    /// for schema mode. If the text is also `!active`, markers are resolved first.
    static Object parse_tool(std::string_view text, const Options& opts = {}) {
        ParseResult r = ::synx::parse(text);
        if (r.mode == Mode::Active) {
            ::synx::resolve(r, opts);
        }
        Value shaped = ::synx::reshape_tool_output(r.root, r.schema);
        if (Object* obj = shaped.as_object_mut()) return std::move(*obj);
        return {};
    }

    /// Convert a Value to canonical JSON.
    static std::string to_json(const Value& v) { return ::synx::to_json(v); }

    /// Serialize Value to SYNX text.
    static std::string stringify(const Value& v) { return ::synx::stringify(v); }

    /// Canonical reformatter for `.synx` text.
    static std::string format(std::string_view text) { return ::synx::format(text); }

    /// Compile text to `.synxb`. Same flags as Rust `Synx::compile`.
    static Result<std::vector<uint8_t>> compile(std::string_view text, bool resolved) {
        ParseResult r = ::synx::parse(text);
        if (resolved && r.mode == Mode::Active) {
            ::synx::resolve(r, Options{});
        }
        return ::synx::compile(r, resolved);
    }

    /// Decompile `.synxb` bytes back into a SYNX text string (with directives).
    static Result<std::string> decompile(const std::vector<uint8_t>& bytes) {
        Result<ParseResult> r = ::synx::decompile(bytes);
        if (!r.ok()) return Result<std::string>::from_error(r.error().message);
        const ParseResult& pr = r.value();
        std::string out;
        if (pr.tool)            out += "!tool\n";
        if (pr.schema)          out += "!schema\n";
        if (pr.llm)             out += "!llm\n";
        if (pr.mode == Mode::Active) out += "!active\n";
        if (pr.locked)          out += "!lock\n";
        if (!out.empty())       out += "\n";
        out += ::synx::stringify(pr.root);
        return Result<std::string>(std::move(out));
    }

    /// True if the bytes start with the `.synxb` magic.
    static bool is_synxb(const std::vector<uint8_t>& data) noexcept {
        return ::synx::is_synxb(data);
    }

    /// Structural diff between two top-level objects.
    static DiffResult diff(const Object& a, const Object& b) {
        return ::synx::diff(a, b);
    }

    /// Diff as Value for JSON.
    static Value diff_to_value(const DiffResult& d) {
        return ::synx::diff_to_value(d);
    }
};

} // namespace synx
