// Value → SYNX text. Port of `serialize` / `format_primitive` in synx-core/src/lib.rs:
// objects go through the SYNXL §14.1 block writer, so the text reads back as
// the same data.
#include "synx/stringify.hpp"

#include "internal.hpp"
#include "numfmt.hpp"

#include <algorithm>
#include <string>
#include <vector>

namespace synx {

namespace {

std::string format_primitive(const Value& v) {
    switch (v.kind()) {
        case Value::Kind::String: return *v.as_string();
        case Value::Kind::Int: return detail::i64_to_string(*v.as_int());
        case Value::Kind::Float: {
            std::string s = detail::display_f64(*v.as_float());
            if (s.find('.') == std::string::npos) s.append(".0");
            return s;
        }
        case Value::Kind::Bool: return *v.as_bool() ? "true" : "false";
        case Value::Kind::Null: return "null";
        case Value::Kind::Array: {
            std::string out = "[";
            const Array& a = *v.as_array();
            for (size_t i = 0; i < a.size(); ++i) {
                if (i > 0) out.append(", ");
                out.append(format_primitive(a[i]));
            }
            out.push_back(']');
            return out;
        }
        case Value::Kind::Object: return "[Object]";
        case Value::Kind::Secret: return "[SECRET]";
    }
    return {};
}

} // namespace

std::string stringify(const Value& val) {
    const Object* map = val.as_object();
    if (!map) return format_primitive(val);
    std::string out;
    out.reserve(256);
    detail::write_synx_object(out, *map, 0, 0);
    return out;
}

} // namespace synx
