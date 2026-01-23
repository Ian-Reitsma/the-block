#include "runtime/Runtime.h"
#include "runtime/CpuContext.h"
#include "runtime/MetalContext.h"

#include <stdexcept>
#include <string>
#include <unordered_map>

namespace orchard::runtime {

MetalContext::MetalContext() {
  device_ = MTLCreateSystemDefaultDevice();
  device_missing_ = device_ == nil;
}

MTLDeviceRef MetalContext::device() const {
  return device_missing_ ? nil : device_;
}

bool MetalContext::has_device() const { return !device_missing_; }

MTLCommandQueueRef MetalContext::acquire_command_queue() {
  if (device_missing_)
    return nil;
  if (!queue_pool_.empty()) {
    id<MTLCommandQueue> queue = queue_pool_.back();
    queue_pool_.pop_back();
    return queue;
  }
  return [device_ newCommandQueue];
}

void MetalContext::return_command_queue(MTLCommandQueueRef queue) {
  if (!device_missing_ && queue)
    queue_pool_.push_back(queue);
}

MTLBlitCommandEncoderRef
MetalContext::acquire_blit_encoder(MTLCommandQueueRef &queue,
                                   MTLCommandBufferRef &cmdBuf) {
  if (device_missing_) {
    queue = nil;
    cmdBuf = nil;
    return nil;
  }
  queue = acquire_command_queue();
  cmdBuf = [queue commandBuffer];
  return [cmdBuf blitCommandEncoder];
}

MetalContext &metal_context() {
  thread_local MetalContext ctx;
  return ctx;
}

using ContextFactory = void *(*)();

namespace {

// Simple registry mapping device names to context factories.
std::unordered_map<std::string, ContextFactory> &registry() {
  static std::unordered_map<std::string, ContextFactory> instance;
  return instance;
}

} // namespace

void register_device(const std::string &name, ContextFactory factory) {
  registry()[name] = factory;
}

void *get_device(const std::string &name) {
  auto it = registry().find(name);
  if (it == registry().end()) {
    return nullptr;
  }
  return it->second();
}

void register_runtime_devices() {
  register_device("metal", []() -> void * { return &metal_context(); });
  register_device("cpu", []() -> void * { return &cpu_context(); });
}

#ifdef __APPLE__
void metal_copy_buffers(MTLBufferRef dstBuf, MTLBufferRef srcBuf,
                        std::size_t bytes) {
  MetalContext &ctx = metal_context();
  if (!ctx.has_device())
    throw std::runtime_error("Metal device unavailable");
  id<MTLCommandQueue> queue = nil;
  id<MTLCommandBuffer> cmd = nil;
  id<MTLBlitCommandEncoder> blit = ctx.acquire_blit_encoder(queue, cmd);
  if (!blit)
    throw std::runtime_error("Metal device unavailable");
  id<MTLBuffer> dst = dstBuf;
  id<MTLBuffer> src = srcBuf;
  [blit copyFromBuffer:src
           sourceOffset:0
               toBuffer:dst
      destinationOffset:0
                   size:bytes];
  [blit endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_copy_cpu_to_metal(MTLBufferRef dstBuf, const void *src,
                             std::size_t bytes) {
  MetalContext &ctx = metal_context();
  if (!ctx.has_device())
    throw std::runtime_error("Metal device unavailable");
  id<MTLBuffer> dst = dstBuf;
  id<MTLBuffer> tmp =
      [ctx.device() newBufferWithBytes:src
                                length:bytes
                               options:MTLResourceStorageModeShared];
  id<MTLCommandQueue> queue = nil;
  id<MTLCommandBuffer> cmd = nil;
  id<MTLBlitCommandEncoder> blit = ctx.acquire_blit_encoder(queue, cmd);
  if (!blit) {
    [tmp release];
    throw std::runtime_error("Metal device unavailable");
  }
  [blit copyFromBuffer:tmp
           sourceOffset:0
               toBuffer:dst
      destinationOffset:0
                   size:bytes];
  [blit endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
  [tmp release];
}

void metal_copy_metal_to_cpu(void *dst, MTLBufferRef srcBuf,
                             std::size_t bytes) {
  MetalContext &ctx = metal_context();
  if (!ctx.has_device())
    throw std::runtime_error("Metal device unavailable");
  id<MTLBuffer> src = srcBuf;
  id<MTLBuffer> tmp =
      [ctx.device() newBufferWithBytesNoCopy:dst
                                      length:bytes
                                     options:MTLResourceStorageModeShared
                                 deallocator:nil];
  id<MTLCommandQueue> queue = nil;
  id<MTLCommandBuffer> cmd = nil;
  id<MTLBlitCommandEncoder> blit = ctx.acquire_blit_encoder(queue, cmd);
  if (!blit) {
    [tmp release];
    throw std::runtime_error("Metal device unavailable");
  }
  [blit copyFromBuffer:src
           sourceOffset:0
               toBuffer:tmp
      destinationOffset:0
                   size:bytes];
  [blit endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
  [tmp release];
}
#endif

} // namespace orchard::runtime

int runtime_stub() { return 0; }
