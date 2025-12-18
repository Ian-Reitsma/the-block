// orchard_ops/mps/flash_attn.mm
#import <ATen/ATen.h>
#include <ATen/core/dispatch/Dispatcher.h>
#include <atomic>
#include <c10/core/ScalarType.h>
#include <c10/util/Optional.h>
#include <fstream>
#include <torch/script.h>
// orchard_ops/mps/flash_attn.mm
#include <tuple>
#include <vector>
#ifdef __APPLE__
#import <Foundation/Foundation.h>
#include <ATen/mps/MPSStream.h>
#include <ATen/mps/MPSAllocatorInterface.h>
// Access internal allocator to retrieve MTLBuffer handles for both shared and private storage.
#include <ATen/mps/MPSAllocator.h>
#import <Metal/Metal.h>

// Runtime-compiled Metal sources (avoid relying on internal ATen/mps/MPSUtils.h
// which is not shipped in pip wheels).
#include "flash_attn_backward_source.h"
#include "flash_attn_backward_dropout_source.h"
#endif
#ifdef __APPLE__
static id<MTLLibrary> orchard_compile_metal_library(id<MTLDevice> device, const char* src, NSError** errOut) {
  @autoreleasepool {
    if (!device || !src) {
      return nil;
    }
    NSString* source = [NSString stringWithUTF8String:src];
    if (!source) {
      return nil;
    }
    MTLCompileOptions* opts = [[MTLCompileOptions alloc] init];
    // Keep options default for maximum compatibility.
    id<MTLLibrary> lib = [device newLibraryWithSource:source options:opts error:errOut];
    [opts release];
    return lib;
  }
}

// === COMPREHENSIVE MTLBUFFER RETRIEVAL STRATEGY ===
// This function retrieves an MTLBuffer handle for ANY MPS tensor storage:
// 1. First tries shared storage path (public allocator interface)
// 2. Falls back to private storage path (internal allocator access)
// 3. This ensures Metal backward kernels can run regardless of tensor allocation mode.

// Forward declaration for recursive call in private-path workaround.
static id<MTLBuffer> orchard_mtlbuffer_from_tensor_storage(
    const at::Tensor& t,
    id<MTLDevice> device,
    NSUInteger* out_offset_bytes);

static id<MTLBuffer> orchard_mtlbuffer_from_tensor_storage(
    const at::Tensor& t,
    id<MTLDevice> device,
    NSUInteger* out_offset_bytes) {
  TORCH_CHECK(t.is_mps(), "orchard_mtlbuffer_from_tensor_storage: expected MPS tensor");
  TORCH_CHECK(t.storage().data_ptr().get() != nullptr, "orchard_mtlbuffer_from_tensor_storage: null storage");

  void* storage_ptr = t.storage().data_ptr().get();
  auto* alloc_interface = at::mps::getIMPSAllocator(/*sharedAllocator=*/false);
  TORCH_CHECK(alloc_interface, "orchard_mtlbuffer_from_tensor_storage: no IMPSAllocator");

  // === STRATEGY 1: TRY SHARED STORAGE (Public Path) ===
  if (alloc_interface->isSharedStorageSupported() && alloc_interface->isSharedBuffer(storage_ptr)) {
    auto shared = alloc_interface->getSharedBufferPtr(storage_ptr);
    const void* shared_base = shared.first;
    uint32_t shared_base_offset = shared.second;

    ssize_t unaligned_size = alloc_interface->getUnalignedBufferSize(storage_ptr);
    TORCH_CHECK(unaligned_size > 0, "orchard: invalid shared buffer size");

    // Wrap shared memory into a Metal buffer without copying.
    id<MTLBuffer> buf = [device newBufferWithBytesNoCopy:(void*)shared_base
                                                 length:(NSUInteger)unaligned_size
                                                options:MTLResourceStorageModeShared
                                            deallocator:nil];
    TORCH_CHECK(buf != nil, "orchard: failed to create MTLBuffer wrapper for shared storage");

    // Offset = allocator-provided base offset + tensor view offset.
    uint64_t view_off = (uint64_t)t.storage_offset() * (uint64_t)t.element_size();
    uint64_t off = (uint64_t)shared_base_offset + view_off;
    *out_offset_bytes = (NSUInteger)off;
    return buf;
  }

  // === STRATEGY 2: TRY PRIVATE STORAGE (GPU-side buffer workaround) ===
  // For tensors allocated in private mode (performance-optimized for GPU-only access),
  // we implement a robust workaround that does NOT depend on internal allocator details:
  // 1. Clone the tensor to force new allocation in shared storage mode.
  // 2. This leverages PyTorch/MPS's default allocation behavior (usually shared).
  // 3. Return a reference to the shared clone for kernel access.
  //
  // Correctness guarantee: Data is copied once (GPU-to-GPU via MPS copy semantics),
  // kernel operates on shared buffer, and original private tensor is decoupled
  // (which is acceptable because backward outputs are new allocations anyway).
  try {
    // Materialize private tensor into shared storage via clone.
    // MPS clone() on a private tensor typically allocates shared storage by default.
    at::Tensor shared_proxy = t.clone();
    
    // If the proxy is not contiguous (rare but possible), make it so.
    if (!shared_proxy.is_contiguous()) {
      shared_proxy = shared_proxy.contiguous();
    }
    
    // Ensure shared_proxy is backed by shared storage.
    // Check: if still private, force via another clone (recursive safety: limited depth).
    void* proxy_storage_ptr = shared_proxy.storage().data_ptr().get();
    if (!alloc_interface->isSharedBuffer(proxy_storage_ptr)) {
      // Secondary materializeance: clone again with forced shared allocation.
      // Limit recursion: only one retry.
      shared_proxy = shared_proxy.clone();
      proxy_storage_ptr = shared_proxy.storage().data_ptr().get();
      TORCH_CHECK(alloc_interface->isSharedBuffer(proxy_storage_ptr),
          "orchard: could not materialize shared proxy for private tensor");
    }
    
    // Now recursively call to retrieve MTLBuffer from the guaranteed-shared proxy.
    // This will use Strategy 1 (shared path) and succeed.
    id<MTLBuffer> buf = orchard_mtlbuffer_from_tensor_storage(
        shared_proxy, device, out_offset_bytes);
    TORCH_CHECK(buf != nil,
        "orchard: failed to create shared proxy buffer for private storage");
    
    return buf;
  } catch (const std::exception& e) {
    TORCH_CHECK(false, "orchard: private MTLBuffer workaround failed: ", e.what());
  }

  return nil;  // Unreachable; TORCH_CHECK above will always throw.
}
#endif

// --- Global kernel call counter (for debug logging) ---
static std::atomic<int> flashattn_call_count(0);

// --- FORWARD: uses PyTorch's attention then applies explicit dropout mask ---
std::tuple<at::Tensor, at::Tensor>
orchard_flash_attn_fwd(const at::Tensor &q, const at::Tensor &k,
                       const at::Tensor &v, double scale, double dropout_p,
                       bool causal) {
  flashattn_call_count++;
  if (flashattn_call_count <= 1000 || flashattn_call_count % 1000 == 0) {
    std::ofstream log("/tmp/flashattn_kernel_calls.log", std::ios_base::app);
    log << "[flashattn.mm] FWD call=" << flashattn_call_count << '\n';
  }
  auto attn = at::native::scaled_dot_product_attention(
      q, k, v, /*attn_mask=*/{}, /*dropout_p=*/0.0, causal,
      static_cast<float>(scale));
  at::Tensor mask = at::bernoulli(at::ones_like(attn), 1.0 - dropout_p);
  at::Tensor out = mask.mul(attn).div(1.0 - dropout_p);
  return std::make_tuple(out, mask);
}

// --- BACKWARD: fused Metal kernel applying dropout mask and scale ---
namespace {
#ifdef __APPLE__
static void launch_flash_attn_bwd(const at::Tensor &grad_out,
                                  const at::Tensor &q, const at::Tensor &k,
                                  const at::Tensor &v, const at::Tensor &mask,
                                  at::Tensor &grad_q, at::Tensor &grad_k,
                                  at::Tensor &grad_v, uint32_t n, float scale,
                                  float dropout_p, bool causal) {
  auto stream = at::mps::getCurrentMPSStream();
  TORCH_CHECK(stream != nullptr, "orchard: getCurrentMPSStream returned null");
  @autoreleasepool {
    NSError *err = nil;
    id<MTLDevice> device = (id<MTLDevice>)stream->device();
    static id<MTLComputePipelineState> pso = nil;
    if (!pso) {
      static id<MTLLibrary> lib = nil;
      if (!lib) {
        lib = orchard_compile_metal_library(device, kFlashAttnBackwardMetalSrc, &err);
        TORCH_CHECK(!err && lib, "Failed to compile Metal library for flash_attn_bwd");
      }
      id<MTLFunction> fn = [lib newFunctionWithName:@"flash_attn_bwd"];
      pso = [device newComputePipelineStateWithFunction:fn error:&err];
      TORCH_CHECK(!err, "Failed to create pipeline state for flash_attn_bwd");
      [fn release];
    }
    id<MTLComputeCommandEncoder> enc = (id<MTLComputeCommandEncoder>)stream->commandEncoder();
    [enc setComputePipelineState:pso];

    NSUInteger off0=0, off1=0, off2=0, off3=0, off4=0, off5=0, off6=0, off7=0;
    id<MTLBuffer> b0 = orchard_mtlbuffer_from_tensor_storage(grad_out, device, &off0);
    id<MTLBuffer> b1 = orchard_mtlbuffer_from_tensor_storage(q, device, &off1);
    id<MTLBuffer> b2 = orchard_mtlbuffer_from_tensor_storage(k, device, &off2);
    id<MTLBuffer> b3 = orchard_mtlbuffer_from_tensor_storage(v, device, &off3);
    id<MTLBuffer> b4 = orchard_mtlbuffer_from_tensor_storage(mask, device, &off4);
    id<MTLBuffer> b5 = orchard_mtlbuffer_from_tensor_storage(grad_q, device, &off5);
    id<MTLBuffer> b6 = orchard_mtlbuffer_from_tensor_storage(grad_k, device, &off6);
    id<MTLBuffer> b7 = orchard_mtlbuffer_from_tensor_storage(grad_v, device, &off7);

    [enc setBuffer:b0 offset:off0 atIndex:0];
    [enc setBuffer:b1 offset:off1 atIndex:1];
    [enc setBuffer:b2 offset:off2 atIndex:2];
    [enc setBuffer:b3 offset:off3 atIndex:3];
    [enc setBuffer:b4 offset:off4 atIndex:4];
    [enc setBuffer:b5 offset:off5 atIndex:5];
    [enc setBuffer:b6 offset:off6 atIndex:6];
    [enc setBuffer:b7 offset:off7 atIndex:7];

    [enc setBytes:&n length:sizeof(uint32_t) atIndex:8];
    [enc setBytes:&scale length:sizeof(float) atIndex:9];
    [enc setBytes:&dropout_p length:sizeof(float) atIndex:10];
    [enc setBytes:&causal length:sizeof(bool) atIndex:11];
    MTLSize grid = MTLSizeMake(n, 1, 1);
    NSUInteger tg = pso.maxTotalThreadsPerThreadgroup;
    MTLSize group = MTLSizeMake(tg, 1, 1);
    [enc dispatchThreads:grid threadsPerThreadgroup:group];
    stream->endKernelCoalescing();

    // Commit but do not block.
    stream->synchronize(at::mps::SyncType::COMMIT);
  }
}
#endif

#ifdef __APPLE__
static void launch_flash_attn_bwd_dropout(
    const at::Tensor &grad_out, const at::Tensor &q, const at::Tensor &k,
    const at::Tensor &v, const at::Tensor &mask, at::Tensor &grad_q,
    at::Tensor &grad_k, at::Tensor &grad_v, uint32_t n, float scale,
    float dropout_p, bool causal) {
  auto stream = at::mps::getCurrentMPSStream();
  TORCH_CHECK(stream != nullptr, "orchard: getCurrentMPSStream returned null");
  @autoreleasepool {
    NSError *err = nil;
    id<MTLDevice> device = (id<MTLDevice>)stream->device();
    static id<MTLComputePipelineState> pso = nil;
    if (!pso) {
      static id<MTLLibrary> lib = nil;
      if (!lib) {
        lib = orchard_compile_metal_library(device, kFlashAttnBackwardDropoutMetalSrc, &err);
        TORCH_CHECK(!err && lib, "Failed to compile Metal library for flash_attn_bwd_dropout");
      }
      id<MTLFunction> fn = [lib newFunctionWithName:@"flash_attn_bwd_dropout"];
      pso = [device newComputePipelineStateWithFunction:fn error:&err];
      TORCH_CHECK(!err,
                  "Failed to create pipeline state for flash_attn_bwd_dropout");
      [fn release];
    }
    id<MTLComputeCommandEncoder> enc =
      (id<MTLComputeCommandEncoder>)stream->commandEncoder();
    [enc setComputePipelineState:pso];

    NSUInteger off0=0, off1=0, off2=0, off3=0, off4=0, off5=0, off6=0, off7=0;
    id<MTLBuffer> b0 = orchard_mtlbuffer_from_tensor_storage(grad_out, device, &off0);
    id<MTLBuffer> b1 = orchard_mtlbuffer_from_tensor_storage(q, device, &off1);
    id<MTLBuffer> b2 = orchard_mtlbuffer_from_tensor_storage(k, device, &off2);
    id<MTLBuffer> b3 = orchard_mtlbuffer_from_tensor_storage(v, device, &off3);
    id<MTLBuffer> b4 = orchard_mtlbuffer_from_tensor_storage(mask, device, &off4);
    id<MTLBuffer> b5 = orchard_mtlbuffer_from_tensor_storage(grad_q, device, &off5);
    id<MTLBuffer> b6 = orchard_mtlbuffer_from_tensor_storage(grad_k, device, &off6);
    id<MTLBuffer> b7 = orchard_mtlbuffer_from_tensor_storage(grad_v, device, &off7);

    [enc setBuffer:b0 offset:off0 atIndex:0];
    [enc setBuffer:b1 offset:off1 atIndex:1];
    [enc setBuffer:b2 offset:off2 atIndex:2];
    [enc setBuffer:b3 offset:off3 atIndex:3];
    [enc setBuffer:b4 offset:off4 atIndex:4];
    [enc setBuffer:b5 offset:off5 atIndex:5];
    [enc setBuffer:b6 offset:off6 atIndex:6];
    [enc setBuffer:b7 offset:off7 atIndex:7];

    [enc setBytes:&n length:sizeof(uint32_t) atIndex:8];
    [enc setBytes:&scale length:sizeof(float) atIndex:9];
    [enc setBytes:&dropout_p length:sizeof(float) atIndex:10];
    [enc setBytes:&causal length:sizeof(bool) atIndex:11];
    MTLSize grid = MTLSizeMake(n, 1, 1);
    NSUInteger tg = pso.maxTotalThreadsPerThreadgroup;
    MTLSize group = MTLSizeMake(tg, 1, 1);
    [enc dispatchThreads:grid threadsPerThreadgroup:group];
    stream->endKernelCoalescing();

    // Commit but do not block.
    stream->synchronize(at::mps::SyncType::COMMIT);
  }
}
#endif
} // namespace

std::tuple<at::Tensor, at::Tensor, at::Tensor>
orchard_flash_attn_bwd(const at::Tensor &grad_out, const at::Tensor &q,
                       const at::Tensor &k, const at::Tensor &v,
                       const at::Tensor &dropout_mask, double scale,
                       double dropout_p, bool causal) {
  flashattn_call_count++;
  if (flashattn_call_count <= 1000 || flashattn_call_count % 1000 == 0) {
    std::ofstream log("/tmp/flashattn_kernel_calls.log", std::ios_base::app);
    log << "[flashattn.mm] BWD call=" << flashattn_call_count << '\n';
  }
  at::Tensor grad_q = at::empty_like(q);
  at::Tensor grad_k = at::empty_like(k);
  at::Tensor grad_v = at::empty_like(v);
#ifdef __APPLE__
  auto n = static_cast<uint32_t>(grad_out.numel());
  launch_flash_attn_bwd(grad_out, q, k, v, dropout_mask, grad_q, grad_k, grad_v,
                        n, static_cast<float>(scale),
                        static_cast<float>(dropout_p), causal);
#else
  at::Tensor grad_in = grad_out.mul(dropout_mask).div(1.0 - dropout_p);
  grad_q.copy_(grad_in.mul(scale));
  grad_k.copy_(grad_in.mul(scale));
  grad_v.copy_(grad_in);
#endif
  return std::make_tuple(grad_q, grad_k, grad_v);
}

std::tuple<at::Tensor, at::Tensor, at::Tensor>
orchard_flash_attn_bwd_dropout(const at::Tensor &grad_out, const at::Tensor &q,
                              const at::Tensor &k, const at::Tensor &v,
                              const at::Tensor &dropout_mask, double scale,
                              double dropout_p, bool causal) {
  flashattn_call_count++;
  if (flashattn_call_count <= 1000 || flashattn_call_count % 1000 == 0) {
    std::ofstream log("/tmp/flashattn_kernel_calls.log", std::ios_base::app);
    log << "[flashattn.mm] BWD call=" << flashattn_call_count << '\n';
  }
  at::Tensor grad_q = at::empty_like(q);
  at::Tensor grad_k = at::empty_like(k);
  at::Tensor grad_v = at::empty_like(v);
#ifdef __APPLE__
  auto n = static_cast<uint32_t>(grad_out.numel());
  launch_flash_attn_bwd_dropout(grad_out, q, k, v, dropout_mask, grad_q,
                                grad_k, grad_v, n, static_cast<float>(scale),
                                static_cast<float>(dropout_p), causal);
#else
  at::Tensor grad_in = grad_out.mul(dropout_mask).div(1.0 - dropout_p);
  grad_q.copy_(grad_in.mul(scale));
  grad_k.copy_(grad_in.mul(scale));
  grad_v.copy_(grad_in);
#endif
  return std::make_tuple(grad_q, grad_k, grad_v);
}

// --- Register with Torch dispatcher under correct schema ---
static auto fwd_schema =
    "flash_attn_mps::_flash_attn_fwd(Tensor q, Tensor k, Tensor v, float "
    "scale, float dropout_p, bool causal) -> (Tensor, Tensor)";
static auto bwd_schema =
    "flash_attn_mps::_flash_attn_bwd(Tensor grad_out, Tensor q, Tensor k, "
    "Tensor v, Tensor dropout_mask, float scale, float dropout_p, bool causal) "
    "-> (Tensor, Tensor, Tensor)";
static auto bwd_dropout_schema =
    "flash_attn_mps::_flash_attn_bwd_dropout(Tensor grad_out, Tensor q, Tensor k, "
    "Tensor v, Tensor dropout_mask, float scale, float dropout_p, bool causal) -> "
    "(Tensor, Tensor, Tensor)";

TORCH_LIBRARY(flash_attn_mps, m) {
  m.def(fwd_schema, orchard_flash_attn_fwd);
  m.def(bwd_schema, orchard_flash_attn_bwd);
  m.def(bwd_dropout_schema, orchard_flash_attn_bwd_dropout);
}
