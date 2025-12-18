// CPU-only runtime implementation used when Metal APIs are unavailable.
#include "runtime/CpuContext.h"
#include "runtime/MetalContext.h"
#include "runtime/Runtime.h"

#include <stdexcept>
#include <string>
#include <unordered_map>

namespace orchard::runtime {
MetalContext::MetalContext() = default;

MTLDeviceRef MetalContext::device() const { return nullptr; }

bool MetalContext::has_device() const { return false; }

MTLCommandQueueRef MetalContext::acquire_command_queue() { return nullptr; }

void MetalContext::return_command_queue(MTLCommandQueueRef) {}

MTLBlitCommandEncoderRef
MetalContext::acquire_blit_encoder(MTLCommandQueueRef &queue,
                                   MTLCommandBufferRef &cmdBuf) {
  queue = nullptr;
  cmdBuf = nullptr;
  return nullptr;
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

void metal_copy_buffers(MTLBufferRef, MTLBufferRef, std::size_t) {
  throw std::runtime_error("Metal device unavailable");
}

void metal_copy_cpu_to_metal(MTLBufferRef, const void *, std::size_t) {
  throw std::runtime_error("Metal device unavailable");
}

void metal_copy_metal_to_cpu(void *, MTLBufferRef, std::size_t) {
  throw std::runtime_error("Metal device unavailable");
}

} // namespace orchard::runtime

int runtime_stub() { return 0; }
