// Copyright APERTURESyndicate.  Native SYNX parser as a UE5 runtime module.
//
// Drop this folder into your project's `Plugins/SynxPlugin/Source/Synx/`.
// The .Build.cs pulls the parser sources directly from `parsers/cpp/{include,src}`
// so a single source-of-truth is shared with the standalone CMake build.

using UnrealBuildTool;
using System.IO;

public class Synx : ModuleRules
{
	public Synx(ReadOnlyTargetRules Target) : base(Target)
	{
		PCHUsage = ModuleRules.PCHUsageMode.UseExplicitOrSharedPCHs;
		CppStandard = CppStandardVersion.Cpp17;
		bUseRTTI = false;
		bEnableExceptions = false;

		// Path layout assumption: this module lives at
		//   <Project>/Plugins/SynxPlugin/Source/Synx/Synx.Build.cs
		// and the SYNX parsers/cpp tree is symlinked or copied alongside as
		//   <Project>/Plugins/SynxPlugin/Source/SynxCore/{include,src}.
		// Adjust the two paths below if your layout differs.
		string ParserRoot = Path.GetFullPath(
			Path.Combine(ModuleDirectory, "..", "SynxCore"));
		string IncludePath = Path.Combine(ParserRoot, "include");
		string SourcePath  = Path.Combine(ParserRoot, "src");

		PublicIncludePaths.Add(IncludePath);
		PrivateIncludePaths.Add(SourcePath);

		// Compile every .cpp in the parsers/cpp/src tree.
		// UE5 normally autodiscovers .cpp under the module directory; we
		// manually add the SynxCore sources because they live outside it.
		foreach (string Cpp in Directory.GetFiles(SourcePath, "*.cpp"))
		{
			// UE5's build system needs the files inside the module folder.
			// One option is to symlink. Another is to drop a thin .cpp stub
			// per source that #includes the real file.  The recommended
			// approach is to include a single "unity" .cpp under Private/
			// which forwards to the real sources — see SynxCoreUnity.cpp.
		}

		PublicDependencyModuleNames.AddRange(new string[]
		{
			"Core",
			"CoreUObject",
			"Engine",
			"zlib",
		});

		PrivateDependencyModuleNames.AddRange(new string[]
		{
			"Json",
			"JsonUtilities",
		});

		PublicDefinitions.Add("SYNX_HAVE_ZLIB=1");
		PublicDefinitions.Add("SYNX_NO_EXCEPTIONS=1");
	}
}
