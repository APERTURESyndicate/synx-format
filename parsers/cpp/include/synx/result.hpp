// Minimal Result<T> / Error pair — used at the public API boundary
// (compile / decompile / file IO) since exceptions are disabled.
#pragma once

#include <string>
#include <utility>

namespace synx {

struct Error {
    std::string message;

    Error() = default;
    explicit Error(std::string msg) : message(std::move(msg)) {}
};

template <typename T>
class Result {
public:
    Result(T value) : has_value_(true), value_(std::move(value)) {}
    Result(Error error) : has_value_(false), error_(std::move(error)) {}

    bool ok() const noexcept { return has_value_; }
    explicit operator bool() const noexcept { return has_value_; }

    const T& value() const& noexcept { return value_; }
    T& value() & noexcept { return value_; }
    T&& value() && noexcept { return std::move(value_); }

    const Error& error() const& noexcept { return error_; }
    Error&& error() && noexcept { return std::move(error_); }

    static Result<T> from_error(std::string msg) { return Result<T>(Error{std::move(msg)}); }

private:
    bool has_value_;
    // Both stored — keeps lifetime simple without union/variant gymnastics.
    T value_{};
    Error error_;
};

} // namespace synx
