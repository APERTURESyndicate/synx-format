#include "SynxBlueprintLibrary.h"

#include "synx/synx.hpp"

#include <string>
#include <vector>

namespace {

std::string ToStd(const FString& S) {
    return std::string(TCHAR_TO_UTF8(*S));
}

FString FromStd(const std::string& S) {
    return FString(UTF8_TO_TCHAR(S.c_str()));
}

} // namespace

FString USynxBlueprintLibrary::SynxParseToJson(const FString& Text) {
    synx::ParseResult r = synx::parse(ToStd(Text));
    return FromStd(synx::to_json(r.root));
}

FString USynxBlueprintLibrary::SynxParseActiveToJson(
    const FString& Text,
    const TArray<FString>& EnvKeys,
    const TArray<FString>& EnvValues) {

    synx::Options opts;
    opts.env.emplace();
    const int32 N = FMath::Min(EnvKeys.Num(), EnvValues.Num());
    for (int32 i = 0; i < N; ++i) {
        opts.env->emplace(ToStd(EnvKeys[i]), ToStd(EnvValues[i]));
    }
    synx::ParseResult r = synx::parse(ToStd(Text));
    if (r.mode == synx::Mode::Active) {
        synx::resolve(r, opts);
    }
    return FromStd(synx::to_json(r.root));
}

FString USynxBlueprintLibrary::SynxFormat(const FString& Text) {
    return FromStd(synx::format(ToStd(Text)));
}

bool USynxBlueprintLibrary::SynxCompile(
    const FString& Text,
    bool bResolveActive,
    TArray<uint8>& OutBytes,
    FString& OutError) {

    auto result = synx::Synx::compile(ToStd(Text), bResolveActive);
    if (!result.ok()) {
        OutError = FromStd(result.error().message);
        return false;
    }
    const auto& bytes = result.value();
    OutBytes.Empty(static_cast<int32>(bytes.size()));
    OutBytes.Append(bytes.data(), static_cast<int32>(bytes.size()));
    return true;
}

bool USynxBlueprintLibrary::SynxDecompile(
    const TArray<uint8>& Bytes,
    FString& OutText,
    FString& OutError) {

    std::vector<uint8_t> v(Bytes.GetData(), Bytes.GetData() + Bytes.Num());
    auto result = synx::Synx::decompile(v);
    if (!result.ok()) {
        OutError = FromStd(result.error().message);
        return false;
    }
    OutText = FromStd(result.value());
    return true;
}
