#pragma once

#include "runtime/MetalContext.h"
#include <cstddef>

namespace orchard::runtime {

void metal_copy_buffers(MTLBufferRef dstBuf, MTLBufferRef srcBuf,
                        std::size_t bytes);
void metal_copy_cpu_to_metal(MTLBufferRef dstBuf, const void *src,
                             std::size_t bytes);
void metal_copy_metal_to_cpu(void *dst, MTLBufferRef srcBuf, std::size_t bytes);

} // namespace orchard::runtime
