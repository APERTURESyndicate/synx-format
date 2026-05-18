// Translation unit anchor for the synx static library.
// All facade methods are inline in include/synx/synx.hpp; this file ensures
// the static library has at least one TU even on stripped CMake configurations.
#include "synx/synx.hpp"

namespace synx {

// Reserved for any future out-of-line facade helpers.

} // namespace synx
