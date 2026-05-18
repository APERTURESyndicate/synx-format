// Blueprint-callable façade over the native SYNX parser.
#pragma once

#include "CoreMinimal.h"
#include "Kismet/BlueprintFunctionLibrary.h"
#include "SynxBlueprintLibrary.generated.h"

UCLASS()
class SYNX_API USynxBlueprintLibrary : public UBlueprintFunctionLibrary
{
	GENERATED_BODY()

public:
	/** Parse a SYNX text and return the result as a JSON string. */
	UFUNCTION(BlueprintCallable, BlueprintPure, Category = "Synx", meta = (Keywords = "synx parse"))
	static FString SynxParseToJson(const FString& Text);

	/**
	 * Parse a SYNX text with `!active` engine resolution.
	 * `EnvKeys` / `EnvValues` are paired arrays acting as the environment map.
	 */
	UFUNCTION(BlueprintCallable, BlueprintPure, Category = "Synx", meta = (Keywords = "synx active"))
	static FString SynxParseActiveToJson(const FString& Text, const TArray<FString>& EnvKeys, const TArray<FString>& EnvValues);

	/** Canonical reformat a SYNX text (sorts keys, normalises indentation). */
	UFUNCTION(BlueprintCallable, BlueprintPure, Category = "Synx")
	static FString SynxFormat(const FString& Text);

	/** Compile a SYNX text to a compact `.synxb` byte array. */
	UFUNCTION(BlueprintCallable, Category = "Synx")
	static bool SynxCompile(const FString& Text, bool bResolveActive, TArray<uint8>& OutBytes, FString& OutError);

	/** Decompile a `.synxb` byte array back to a SYNX text string. */
	UFUNCTION(BlueprintCallable, Category = "Synx")
	static bool SynxDecompile(const TArray<uint8>& Bytes, FString& OutText, FString& OutError);
};
