// Internal declarations shared by engine.cpp and engine_markers.cpp.
// Port of crates/synx-core/src/engine.rs.
#pragma once

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

#include "synx/key_index.hpp"
#include "synx/options.hpp"
#include "synx/parse_result.hpp"
#include "synx/value.hpp"

namespace synx {
namespace detail {

constexpr size_t kMaxCalcExprLen = 4096;
constexpr size_t kMaxCalcResolvedLen = 64 * 1024;
constexpr uint64_t kMaxFileSize = 10ull * 1024 * 1024;
constexpr size_t kDefaultMaxIncludeDepth = 16;
constexpr size_t kMaxResolveDepth = 512;
constexpr size_t kMaxScratchString = 4u * 1024 * 1024;
/// Included documents one top-level resolve may read.
constexpr size_t kMaxIncludeReads = 1024;
/// Bytes one top-level resolve may read through `:include`, `!include`, `!use`
/// and `:watch`: every file is capped at 10 MiB, but a document may name the
/// same file from a thousand keys.
constexpr uint64_t kMaxResolveReadBytes = 64ull * 1024 * 1024;
/// Bytes the markers of one top-level resolve may add to the document: values
/// copied by `:alias`, `:ref`, `:spam` and `:inherit`, text built by `{…}`,
/// `:replace`, `:join`, `:format` and `:prompt`. synx-core's costs, so a
/// document runs out at the same key in every implementation.
constexpr size_t kMaxResolveExpansionBytes = 64u * 1024 * 1024;
/// What one copied value costs against the expansion budget, text aside.
constexpr size_t kValueCost = 128;
inline constexpr const char* kLimitErr = "LIMIT_ERR: resolution budget exceeded";
inline constexpr const char* kNestingErr = "NESTING_ERR: maximum object nesting depth exceeded";

// Bits for the built-in markers `apply_markers` dispatches on.
namespace mk {
constexpr uint32_t ALIAS = 1u << 0;
constexpr uint32_t CALC = 1u << 1;
constexpr uint32_t CLAMP = 1u << 2;
constexpr uint32_t DEFAULT = 1u << 3;
constexpr uint32_t ENV = 1u << 4;
constexpr uint32_t FALLBACK = 1u << 5;
constexpr uint32_t FORMAT = 1u << 6;
constexpr uint32_t GEO = 1u << 7;
constexpr uint32_t I18N = 1u << 8;
constexpr uint32_t IMPORT = 1u << 9;
constexpr uint32_t INCLUDE = 1u << 10;
constexpr uint32_t JOIN = 1u << 11;
constexpr uint32_t MAP = 1u << 12;
constexpr uint32_t ONCE = 1u << 13;
constexpr uint32_t PROMPT = 1u << 14;
constexpr uint32_t RANDOM = 1u << 15;
constexpr uint32_t REF = 1u << 16;
constexpr uint32_t REPLACE = 1u << 17;
constexpr uint32_t ROUND = 1u << 18;
constexpr uint32_t SECRET = 1u << 19;
constexpr uint32_t SORT = 1u << 20;
constexpr uint32_t SPAM = 1u << 21;
constexpr uint32_t SPLIT = 1u << 22;
constexpr uint32_t SUM = 1u << 23;
constexpr uint32_t UNIQUE = 1u << 24;
constexpr uint32_t VERSION = 1u << 25;
constexpr uint32_t WATCH = 1u << 26;
} // namespace mk

/// Classify a marker chain once (arguments and unknown names contribute nothing).
uint32_t marker_bits(const std::vector<std::string>& markers) noexcept;

/// Key lookup in an object that keeps neither its keys nor its entry buffer
/// changing while the lookup lives (true during resolution: markers replace
/// values, never add or remove keys). Linear below a threshold, hashed above.
class ObjectLookup {
public:
    explicit ObjectLookup(const Object& obj) : obj_(&obj) {}
    const Value* get(std::string_view key) const;
    Value* get_mut(std::string_view key) const { return const_cast<Value*>(get(key)); }
    const Object& object() const noexcept { return *obj_; }

private:
    const Object* obj_;
    mutable KeyIndex index_;
};

/// Text of a value for interpolation, `:join`, enum checks… (Rust `value_to_string`).
std::string value_to_string(const Value& v);
/// Int or Float as f64 (Rust `value_as_number`).
bool value_as_number(const Value& v, double& out) noexcept;
/// Rust `engine::cast_primitive`.
Value cast_primitive(std::string_view val);
/// `(string)` / `[type:string]` keep outside text as text.
bool wants_string(const Meta& m);
/// `root.key` directly, then a dotted path through objects (Rust `deep_get`).
const Value* deep_get(const Value& root, std::string_view path, const ObjectLookup* root_lookup);

/// Path sandbox: `Err` text, or the file path inside `base`.
struct JailResult {
    bool ok = false;
    std::filesystem::path path;
    std::string error;
};
JailResult jail_path(std::string_view base, std::string_view file_path);
/// `SECURITY: file too large …`, or empty.
std::string check_file_size(const std::filesystem::path& p);
/// Read a UTF-8 text file; on failure `error` holds Rust's normalised message.
bool read_text(const std::filesystem::path& p, std::string_view ctx, std::string& out, std::string& error);
std::filesystem::path path_from_utf8(std::string_view s);
std::string path_to_utf8(const std::filesystem::path& p);
/// Parse, and resolve an `!active` file with the include depth bumped.
Value load_synx_file(const std::string& text, const std::filesystem::path& file, const Options& options);

/// Read a file for `:include`, `!include`, `!use` or `:watch` within the
/// resolve's read budget; `error` is synx-core's portable text.
bool read_budgeted(const std::filesystem::path& p, std::string_view shown, std::string& out, std::string& error);
/// Read and resolve the document at `full` (`shown`: as the document wrote it),
/// or refuse a file already being resolved (a cycle) and reads past the budget.
bool include_file(const std::filesystem::path& full, std::string_view shown, const Options& options, Value& out,
                  std::string& error);

/// Take `bytes` from the expansion budget; false once it is spent (and it stays spent).
bool charge_expansion(size_t bytes) noexcept;
/// Bytes of the expansion budget not yet spent.
size_t expansion_room() noexcept;
/// What copying a value costs, and how many levels of objects and lists it nests.
std::pair<size_t, size_t> value_cost(const Value& v);
/// A copy of `v` for an object `depth` levels below the root, or the error left instead.
Value budgeted_copy(const Value& v, size_t depth);
/// A value read from a file for an object `depth` levels below the root.
Value placed_document(Value v, size_t depth, bool secret);
/// `s` is an error the engine left in place of a value.
bool is_engine_error(std::string_view s) noexcept;
/// Make a resolved value a secret: scalars by their text, lists and groups leaf
/// by leaf; null, an existing secret and an engine error are left as they are.
void mask_secret_value(Value& v);
/// Text from outside the document as a value; under `:secret` a number or a
/// bool keeps its text (`00123`).
Value external_value(std::string text, bool as_string, bool secret);

/// Rust `has_nested_quantifier`: `(a+)+` shapes are refused everywhere.
bool has_nested_quantifier(std::string_view pattern);
/// Does `s` fail `[pattern:…]`? Refused / invalid patterns never reject.
bool pattern_rejects(const std::string& pattern, const std::string& s);

/// The error a value earns against its constraints, or nullopt.
std::optional<std::string> constraint_violation(std::string_view key, const Value& val, const Constraints& c);

/// Key substitution facts about the root that hold for one resolution.
struct CalcScope {
    std::vector<std::string> root_special;
};
bool is_calc_special_key(std::string_view k) noexcept;

/// What the keys under `:secret` held before the pre-pass masked them, by
/// object path and key.
using SecretOriginals = std::unordered_map<std::string, std::unordered_map<std::string, Value>>;

/// [`mask_secret_literals`] for the object or list at `path`.
void mask_secret_subtree(Value& child, const MetadataTree& metadata, std::string& path, SecretOriginals& originals);

/// State for one `resolve` call.
struct Resolver {
    const Options& options;
    Value& root;
    const ObjectLookup& root_lookup;
    const MetadataTree& metadata;
    const IndexMap<Value>& includes;
    /// Root `:include` keys the pre-pass already resolved, as `[path, root]`;
    /// kept apart from `includes`, whose size `{key:include}` reads.
    const IndexMap<Value>& preresolved;
    const CalcScope& calc_scope;
    SecretOriginals& secret_originals;

    void resolve_value(Value& value, std::string& path, size_t depth);
    /// `depth`: how many levels below the root `map` sits.
    void apply_markers(Object& map, size_t index, const ObjectLookup& local, const Meta& meta,
                       const std::string& path, size_t depth);
    /// nullopt when the text would pass `cap` bytes.
    std::optional<std::string> resolve_interpolation(std::string_view tpl, const ObjectLookup& local,
                                                     size_t cap) const;
};

/// JSON text → Value (`:watch` on a `.json` file); nullopt when it is not JSON.
std::optional<Value> parse_json(std::string_view text);

} // namespace detail
} // namespace synx
