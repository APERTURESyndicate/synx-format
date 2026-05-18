// Unity TU — pulls every SYNX parser source into the UE5 module compile.
//
// UE5's build system only autodiscovers .cpp files inside the module folder.
// Rather than copy or symlink the 11 parser sources, we use a single unity
// file that includes them. Each TU still gets full -fno-exceptions / -fno-rtti.
#include "value.cpp"        // NOLINT
#include "parser.cpp"       // NOLINT
#include "engine.cpp"       // NOLINT
#include "engine_markers.cpp" // NOLINT
#include "calc.cpp"         // NOLINT
#include "json.cpp"         // NOLINT
#include "stringify.cpp"    // NOLINT
#include "formatter.cpp"    // NOLINT
#include "diff.cpp"         // NOLINT
#include "binary.cpp"       // NOLINT
#include "synx.cpp"         // NOLINT
