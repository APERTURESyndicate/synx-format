// Structural diff. Mirrors synx-core/src/diff.rs.
#include "synx/diff.hpp"

#include <algorithm>

namespace synx {

namespace {

const Value* find(const Object& obj, const std::string& key) noexcept {
    for (const auto& p : obj) {
        if (p.key == key) return &p.value;
    }
    return nullptr;
}

bool deep_equal(const Value& a, const Value& b) {
    return a.equals(b);
}

} // namespace

DiffResult diff(const Object& a, const Object& b) {
    DiffResult d;

    for (const auto& pa : a) {
        const Value* bv = find(b, pa.key);
        if (!bv) {
            d.removed.push_back(Pair{pa.key, pa.value});
        } else if (deep_equal(pa.value, *bv)) {
            d.unchanged.push_back(pa.key);
        } else {
            d.changed.push_back({pa.key, DiffChange{pa.value, *bv}});
        }
    }
    for (const auto& pb : b) {
        if (!find(a, pb.key)) {
            d.added.push_back(Pair{pb.key, pb.value});
        }
    }
    std::sort(d.unchanged.begin(), d.unchanged.end());
    return d;
}

Value diff_to_value(const DiffResult& d) {
    Object root;
    root.push_back(Pair{"added",   Value::make_object(d.added)});
    root.push_back(Pair{"removed", Value::make_object(d.removed)});

    Object changed_obj;
    for (const auto& entry : d.changed) {
        Object inner;
        inner.push_back(Pair{"from", entry.second.from});
        inner.push_back(Pair{"to",   entry.second.to});
        changed_obj.push_back(Pair{entry.first, Value::make_object(std::move(inner))});
    }
    root.push_back(Pair{"changed", Value::make_object(std::move(changed_obj))});

    Array unchanged_arr;
    unchanged_arr.reserve(d.unchanged.size());
    for (const auto& s : d.unchanged) {
        unchanged_arr.push_back(Value::make_string(s));
    }
    root.push_back(Pair{"unchanged", Value::make_array(std::move(unchanged_arr))});

    return Value::make_object(std::move(root));
}

} // namespace synx
