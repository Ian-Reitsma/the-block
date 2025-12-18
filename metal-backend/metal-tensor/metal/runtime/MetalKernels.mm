#include "MetalKernels.h"
#include "CpuContext.h"
#include "MetalContext.h"

#include <array>
#include <cstring>
#include <fstream>
#include <stdexcept>
#include <string>

#ifdef __APPLE__
#include <Foundation/Foundation.h>
#endif

namespace orchard::runtime {

namespace {
std::string load_kernel_src(const char *file) {
  std::string path = std::string(ORCHARD_KERNEL_DIR) + "/" + file;
  std::ifstream ifs(path);
  if (!ifs.good()) {
    throw std::runtime_error("Failed to open Metal kernel: " + path);
  }
  return std::string((std::istreambuf_iterator<char>(ifs)),
                     std::istreambuf_iterator<char>());
}
} // namespace

#ifdef __APPLE__
void metal_add(const float *a, const float *b, float *c,
               const std::int64_t *shape, const std::int64_t *astrides,
               const std::int64_t *bstrides, std::uint32_t dims,
               std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("add.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "add.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"add_arrays"];
    if (!fn) {
      std::string msg = "add_arrays function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "add pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)b offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)c offset:0 atIndex:2];
  id<MTLBuffer> shapeBuf =
      [ctx.device() newBufferWithBytes:shape
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  id<MTLBuffer> aBuf =
      [ctx.device() newBufferWithBytes:astrides
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  id<MTLBuffer> bBuf =
      [ctx.device() newBufferWithBytes:bstrides
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  [enc setBuffer:shapeBuf offset:0 atIndex:3];
  [enc setBuffer:aBuf offset:0 atIndex:4];
  [enc setBuffer:bBuf offset:0 atIndex:5];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  [shapeBuf release];
  [aBuf release];
  [bBuf release];
  ctx.return_command_queue(queue);
}

void metal_mul(const float *a, const float *b, float *c,
               const std::int64_t *shape, const std::int64_t *astrides,
               const std::int64_t *bstrides, std::uint32_t dims,
               std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("mul.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "mul.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"mul_arrays"];
    if (!fn) {
      std::string msg = "mul_arrays function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "mul pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)b offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)c offset:0 atIndex:2];
  id<MTLBuffer> shapeBuf =
      [ctx.device() newBufferWithBytes:shape
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  id<MTLBuffer> aBuf =
      [ctx.device() newBufferWithBytes:astrides
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  id<MTLBuffer> bBuf =
      [ctx.device() newBufferWithBytes:bstrides
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  [enc setBuffer:shapeBuf offset:0 atIndex:3];
  [enc setBuffer:aBuf offset:0 atIndex:4];
  [enc setBuffer:bBuf offset:0 atIndex:5];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  [shapeBuf release];
  [aBuf release];
  [bBuf release];
  ctx.return_command_queue(queue);
}
void metal_div(const float *a, const float *b, float *c,
               const std::int64_t *shape, const std::int64_t *astrides,
               const std::int64_t *bstrides, std::uint32_t dims, std::size_t n,
               bool safe) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("div.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "div.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"div_arrays"];
    if (!fn) {
      std::string msg = "div_arrays function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "div pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)b offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)c offset:0 atIndex:2];
  id<MTLBuffer> shapeBuf =
      [ctx.device() newBufferWithBytes:shape
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  id<MTLBuffer> aBuf =
      [ctx.device() newBufferWithBytes:astrides
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  id<MTLBuffer> bBuf =
      [ctx.device() newBufferWithBytes:bstrides
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  [enc setBuffer:shapeBuf offset:0 atIndex:3];
  [enc setBuffer:aBuf offset:0 atIndex:4];
  [enc setBuffer:bBuf offset:0 atIndex:5];
  int s = safe ? 1 : 0;
  [enc setBytes:&s length:sizeof(int) atIndex:6];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  [shapeBuf release];
  [aBuf release];
  [bBuf release];
  ctx.return_command_queue(queue);
}

void metal_div_scalar(const float *a, float scalar, float *out, std::size_t n,
                      bool safe) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("div.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "div.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"div_scalar"];
    if (!fn) {
      std::string msg = "div_scalar function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "div_scalar pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:0];
  [enc setBytes:&scalar length:sizeof(float) atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)out offset:0 atIndex:2];
  int s = safe ? 1 : 0;
  [enc setBytes:&s length:sizeof(int) atIndex:3];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_mul_backward_a(const float *g, const float *b, float *ga,
                          std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("mul_backward.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "mul_backward.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"mul_backward_a"];
    if (!fn) {
      std::string msg = "mul_backward_a function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "mul_backward_a pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)g offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)b offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)ga offset:0 atIndex:2];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_mul_backward_b(const float *g, const float *a, float *gb,
                          std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("mul_backward.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "mul_backward.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"mul_backward_b"];
    if (!fn) {
      std::string msg = "mul_backward_b function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "mul_backward_b pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)g offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)gb offset:0 atIndex:2];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}
void metal_div_backward_a(const float *g, const float *b, float *ga,
                          std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("div.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "div.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"div_backward_a"];
    if (!fn) {
      std::string msg = "div_backward_a function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "div_backward_a pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)g offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)b offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)ga offset:0 atIndex:2];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_div_backward_b(const float *g, const float *a, const float *b,
                          float *gb, std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("div.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "div.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"div_backward_b"];
    if (!fn) {
      std::string msg = "div_backward_b function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "div_backward_b pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)g offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)b offset:0 atIndex:2];
  [enc setBuffer:(__bridge id<MTLBuffer>)gb offset:0 atIndex:3];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_matmul(const float *a, const float *b, float *c, std::size_t m,
                  std::size_t n, std::size_t k) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("matmul.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "matmul.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"matmul_kernel"];
    if (!fn) {
      std::string msg = "matmul_kernel function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "matmul pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)b offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)c offset:0 atIndex:2];
  uint32_t mm = static_cast<uint32_t>(m);
  uint32_t nn = static_cast<uint32_t>(n);
  uint32_t kk = static_cast<uint32_t>(k);
  [enc setBytes:&mm length:sizeof(uint32_t) atIndex:3];
  [enc setBytes:&nn length:sizeof(uint32_t) atIndex:4];
  [enc setBytes:&kk length:sizeof(uint32_t) atIndex:5];
  MTLSize grid = MTLSizeMake(m * n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_reduce_sum(const float *a, float *out, std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("reduce_sum.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "reduce_sum.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"reduce_sum"];
    if (!fn) {
      std::string msg = "reduce_sum function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "reduce_sum pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)out offset:0 atIndex:1];
  uint32_t nn = static_cast<uint32_t>(n);
  [enc setBytes:&nn length:sizeof(uint32_t) atIndex:2];
  MTLSize grid = MTLSizeMake(1, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_mean(const float *a, float *out, std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("mean.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "mean.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"mean"];
    if (!fn) {
      std::string msg = "mean function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "mean pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)out offset:0 atIndex:1];
  uint32_t nn = static_cast<uint32_t>(n);
  [enc setBytes:&nn length:sizeof(uint32_t) atIndex:2];
  MTLSize grid = MTLSizeMake(1, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

// Parameters follow (m, n, k)
void metal_matmul_backward_a(const float *g, const float *b, float *ga,
                             std::size_t m, std::size_t n, std::size_t k) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("matmul_backward.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "matmul_backward.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"matmul_backward_a"];
    if (!fn) {
      std::string msg = "matmul_backward_a function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "matmul_backward_a pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)g offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)b offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)ga offset:0 atIndex:2];
  uint32_t mm = static_cast<uint32_t>(m);
  uint32_t nn = static_cast<uint32_t>(n);
  uint32_t kk = static_cast<uint32_t>(k);
  [enc setBytes:&mm length:sizeof(uint32_t) atIndex:3];
  [enc setBytes:&nn length:sizeof(uint32_t) atIndex:4];
  [enc setBytes:&kk length:sizeof(uint32_t) atIndex:5];
  MTLSize grid = MTLSizeMake(m * k, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

// Parameters follow (m, n, k)
void metal_matmul_backward_b(const float *g, const float *a, float *gb,
                             std::size_t m, std::size_t n, std::size_t k) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("matmul_backward.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "matmul_backward.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"matmul_backward_b"];
    if (!fn) {
      std::string msg = "matmul_backward_b function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "matmul_backward_b pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)g offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:1];
  [enc setBuffer:(__bridge id<MTLBuffer>)gb offset:0 atIndex:2];
  uint32_t mm = static_cast<uint32_t>(m);
  uint32_t nn = static_cast<uint32_t>(n);
  uint32_t kk = static_cast<uint32_t>(k);
  [enc setBytes:&mm length:sizeof(uint32_t) atIndex:3];
  [enc setBytes:&nn length:sizeof(uint32_t) atIndex:4];
  [enc setBytes:&kk length:sizeof(uint32_t) atIndex:5];
  MTLSize grid = MTLSizeMake(k * n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_transpose_backward(const float *g, float *out, std::size_t m,
                              std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("transpose_backward.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "transpose_backward.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"transpose_backward"];
    if (!fn) {
      std::string msg = "transpose_backward function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "transpose_backward pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)g offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)out offset:0 atIndex:1];
  uint32_t mm = static_cast<uint32_t>(m);
  uint32_t nn = static_cast<uint32_t>(n);
  [enc setBytes:&mm length:sizeof(uint32_t) atIndex:2];
  [enc setBytes:&nn length:sizeof(uint32_t) atIndex:3];
  MTLSize grid = MTLSizeMake(m * n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_fill(float *out, float value, std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("fill.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "fill.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"fill_value"];
    if (!fn) {
      std::string msg = "fill_value function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "fill pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)out offset:0 atIndex:0];
  [enc setBytes:&value length:sizeof(float) atIndex:1];
  uint32_t nn = static_cast<uint32_t>(n);
  [enc setBytes:&nn length:sizeof(uint32_t) atIndex:2];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  ctx.return_command_queue(queue);
}

void metal_reduce_sum_axis(const float *a, float *out,
                           const std::int64_t *shape,
                           const std::int64_t *strides, std::uint32_t dims,
                           std::uint32_t axis_len, std::uint32_t axis,
                           std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("reduce_sum_axis.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "reduce_sum_axis.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"reduce_sum_axis"];
    if (!fn) {
      std::string msg = "reduce_sum_axis function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "reduce_sum_axis pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)out offset:0 atIndex:1];
  id<MTLBuffer> shapeBuf =
      [ctx.device() newBufferWithBytes:shape
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  id<MTLBuffer> strideBuf =
      [ctx.device() newBufferWithBytes:strides
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  [enc setBuffer:shapeBuf offset:0 atIndex:2];
  [enc setBuffer:strideBuf offset:0 atIndex:3];
  [enc setBytes:&axis_len length:sizeof(uint32_t) atIndex:4];
  [enc setBytes:&axis length:sizeof(uint32_t) atIndex:5];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  [shapeBuf release];
  [strideBuf release];
  ctx.return_command_queue(queue);
}

void metal_mean_axis(const float *a, float *out, const std::int64_t *shape,
                     const std::int64_t *strides, std::uint32_t dims,
                     std::uint32_t axis_len, std::uint32_t axis,
                     std::size_t n) {
  static id<MTLComputePipelineState> pipeline = nil;
  MetalContext &ctx = metal_context();
  if (!ctx.device())
    throw std::runtime_error("Metal device unavailable");
  if (!pipeline) {
    std::string src = load_kernel_src("mean_axis.metal");
    NSString *nsSrc = [[NSString alloc] initWithBytes:src.data()
                                               length:src.size()
                                             encoding:NSUTF8StringEncoding];
    NSError *err = nil;
    id<MTLLibrary> lib = [ctx.device() newLibraryWithSource:nsSrc
                                                    options:nil
                                                      error:&err];
    if (err || !lib) {
      std::string msg = "mean_axis.metal: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    id<MTLFunction> fn = [lib newFunctionWithName:@"mean_axis"];
    if (!fn) {
      std::string msg = "mean_axis function missing";
      NSLog(@"%s", msg.c_str());
      [lib release];
      [nsSrc release];
      throw std::runtime_error(msg);
    }
    pipeline = [ctx.device() newComputePipelineStateWithFunction:fn error:&err];
    [fn release];
    [lib release];
    if (err || !pipeline) {
      std::string msg = "mean_axis pipeline: ";
      if (err) {
        msg += [[err localizedDescription] UTF8String];
        NSLog(@"%@", err);
      }
      throw std::runtime_error(msg);
    }
    [nsSrc release];
  }
  id<MTLCommandQueue> queue = ctx.acquire_command_queue();
  id<MTLCommandBuffer> cmd = [queue commandBuffer];
  id<MTLComputeCommandEncoder> enc = [cmd computeCommandEncoder];
  [enc setComputePipelineState:pipeline];
  [enc setBuffer:(__bridge id<MTLBuffer>)(const void *)a offset:0 atIndex:0];
  [enc setBuffer:(__bridge id<MTLBuffer>)out offset:0 atIndex:1];
  id<MTLBuffer> shapeBuf =
      [ctx.device() newBufferWithBytes:shape
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  id<MTLBuffer> strideBuf =
      [ctx.device() newBufferWithBytes:strides
                                length:sizeof(std::int64_t) * dims
                               options:MTLResourceStorageModeShared];
  [enc setBuffer:shapeBuf offset:0 atIndex:2];
  [enc setBuffer:strideBuf offset:0 atIndex:3];
  [enc setBytes:&axis_len length:sizeof(uint32_t) atIndex:4];
  [enc setBytes:&axis length:sizeof(uint32_t) atIndex:5];
  MTLSize grid = MTLSizeMake(n, 1, 1);
  MTLSize thread = MTLSizeMake(1, 1, 1);
  [enc dispatchThreads:grid threadsPerThreadgroup:thread];
  [enc endEncoding];
  [cmd commit];
  [cmd waitUntilCompleted];
  [shapeBuf release];
  [strideBuf release];
  ctx.return_command_queue(queue);
}

#else

void metal_div_backward_a(const float *g, const float *b, float *ga,
                          std::size_t n) {
  for (std::size_t i = 0; i < n; ++i)
    ga[i] = g[i] / b[i];
}

void metal_div_backward_b(const float *g, const float *a, const float *b,
                          float *gb, std::size_t n) {
  for (std::size_t i = 0; i < n; ++i)
    gb[i] = -g[i] * a[i] / (b[i] * b[i]);
}

#endif

} // namespace orchard::runtime
