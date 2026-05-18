// Module entry point.
#include "CoreMinimal.h"
#include "Modules/ModuleManager.h"

class FSynxModule : public IModuleInterface
{
public:
	virtual void StartupModule() override {}
	virtual void ShutdownModule() override {}
};

IMPLEMENT_MODULE(FSynxModule, Synx)
