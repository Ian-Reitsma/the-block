#pragma once

#include <vector>

namespace orchard::runtime {

#if defined(__APPLE__) && defined(__OBJC__)
#include <Metal/Metal.h>
using MTLDeviceRef = id<MTLDevice>;
using MTLCommandQueueRef = id<MTLCommandQueue>;
using MTLCommandBufferRef = id<MTLCommandBuffer>;
using MTLBlitCommandEncoderRef = id<MTLBlitCommandEncoder>;
using MTLBufferRef = id<MTLBuffer>;
#else
using MTLDeviceRef = void *;
using MTLCommandQueueRef = void *;
using MTLCommandBufferRef = void *;
using MTLBlitCommandEncoderRef = void *;
using MTLBufferRef = void *;
#endif

class MetalContext {
public:
  MetalContext();

  MTLDeviceRef device() const;
  bool has_device() const;

  MTLCommandQueueRef acquire_command_queue();
  void return_command_queue(MTLCommandQueueRef queue);
  MTLBlitCommandEncoderRef acquire_blit_encoder(MTLCommandQueueRef &queue,
                                                MTLCommandBufferRef &cmdBuf);

private:
  MTLDeviceRef device_ = MTLDeviceRef{};
  bool device_missing_ = true;
  std::vector<MTLCommandQueueRef> queue_pool_;
};

MetalContext &metal_context();

} // namespace orchard::runtime
