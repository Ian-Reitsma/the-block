#pragma once

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <sstream>

#include "common/Profiling.h"

#ifdef __APPLE__
#ifdef __OBJC__
#import <Foundation/Foundation.h>
#import <IOSurface/IOSurface.h>
#import <Metal/Metal.h>
#endif
#endif

namespace orchard::runtime {

class Allocator {
public:
  virtual ~Allocator() = default;
  virtual void *allocate(std::size_t bytes, const char *label) = 0;
  virtual void deallocate(void *ptr, const char *label) = 0;
};

class CpuAllocator : public Allocator {
public:
  void *allocate(std::size_t bytes, const char *label) override {
    void *p = nullptr;
    posix_memalign(&p, 64, bytes);
    std::ostringstream oss;
    oss << "alloc " << label << ' ' << bytes << ' ' << p;
    orchard::tensor_profile_log(oss.str());
    return p;
  }
  void deallocate(void *ptr, const char *label) override {
    std::ostringstream oss;
    oss << "free " << label << ' ' << ptr;
    orchard::tensor_profile_log(oss.str());
    free(ptr);
  }
};

class MetalAllocator : public Allocator {
public:
  MetalAllocator();
  void *allocate(std::size_t bytes, const char *label) override;
  void deallocate(void *ptr, const char *label) override;

private:
#ifdef __OBJC__
  id<MTLDevice> device_{nil};
#endif
};

inline MetalAllocator::MetalAllocator() {
#ifdef __OBJC__
  device_ = MTLCreateSystemDefaultDevice();
  if (!device_) {
    orchard::tensor_profile_log("error missing Metal device");
  }
#endif
}

inline void *MetalAllocator::allocate(std::size_t bytes, const char *label) {
#ifdef __OBJC__
  if (!device_) {
    orchard::tensor_profile_log("error missing Metal device");
    return nullptr;
  }
  id<MTLBuffer> buffer = nil;
  if (bytes > (16 << 20)) {
    const void *keys[] = {(const void *)kIOSurfaceWidth,
                          (const void *)kIOSurfaceHeight,
                          (const void *)kIOSurfaceBytesPerElement};
    int width = bytes;
    int height = 1;
    int bpe = 1;
    const void *values[] = {
        CFNumberCreate(nullptr, kCFNumberSInt32Type, &width),
        CFNumberCreate(nullptr, kCFNumberSInt32Type, &height),
        CFNumberCreate(nullptr, kCFNumberSInt32Type, &bpe),
    };
    CFDictionaryRef dict = CFDictionaryCreate(nullptr, keys, values, 3,
                                              &kCFTypeDictionaryKeyCallBacks,
                                              &kCFTypeDictionaryValueCallBacks);
    IOSurfaceRef surface = IOSurfaceCreate(dict);
    for (int i = 0; i < 3; ++i)
      CFRelease(values[i]);
    CFRelease(dict);
    buffer = [device_ newBufferWithIOSurface:surface
                                     options:MTLResourceStorageModeShared
                                      offset:0
                                      length:bytes];
    [buffer setPurgeableState:MTLPurgeableStateKeepCurrent];
    CFRelease(surface);
  } else {
    buffer = [device_ newBufferWithLength:bytes
                                  options:MTLResourceStorageModeShared];
  }
  buffer.label = [[NSString alloc] initWithUTF8String:label];
  void *p = (__bridge_retained void *)buffer;
  std::ostringstream oss;
  oss << "alloc " << label << ' ' << bytes << ' ' << p;
  orchard::tensor_profile_log(oss.str());
  return p;
#else
  void *p = nullptr;
  posix_memalign(&p, 64, bytes);
  std::ostringstream oss;
  oss << "alloc " << label << ' ' << bytes << ' ' << p;
  orchard::tensor_profile_log(oss.str());
  return p;
#endif
}

inline void MetalAllocator::deallocate(void *ptr, const char *label) {
  std::ostringstream oss;
  oss << "free " << label << ' ' << ptr;
  orchard::tensor_profile_log(oss.str());
#ifdef __OBJC__
  id<MTLBuffer> buffer = (__bridge_transfer id<MTLBuffer>)ptr;
  buffer = nil;
#else
  free(ptr);
#endif
}

} // namespace orchard::runtime
