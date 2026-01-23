#include "Debug.h"
#include "Storage.h"

#include <mutex>
#include <sstream>

namespace orchard::core::tensor {

void dump_live_tensors() {
  std::lock_guard<std::mutex> g(live_storage_mutex);
  for (auto *st : live_storages) {
    std::ostringstream oss;
    oss << "live " << st->label << ' ' << st->nbytes;
    orchard::tensor_profile_log(oss.str());
  }
}

} // namespace orchard::core::tensor
