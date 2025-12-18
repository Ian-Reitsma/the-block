add_library(orchard_core STATIC
  metal/core/core.cpp
  metal/core/autograd/Node.cpp
  metal/core/autograd/AddBackward.cpp
  metal/core/autograd/MatmulBackward.cpp
  metal/core/autograd/MulBackward.cpp
  metal/core/autograd/DivBackward.cpp
  metal/core/autograd/DivScalarBackward.cpp
  metal/core/autograd/MeanBackward.cpp
  metal/core/autograd/SumBackward.cpp
  metal/core/autograd/TransposeBackward.cpp
  metal/core/autograd/ViewBackward.cpp
  metal/core/tensor/Tensor.cpp
  metal/core/tensor/Debug.cpp
)

target_include_directories(orchard_core PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/metal)
if(NOT APPLE)
  target_link_libraries(orchard_core PUBLIC uuid)
endif()
if(APPLE)
  target_link_libraries(orchard_core PUBLIC "-framework Accelerate")
endif()

add_library(orchard_metal STATIC)

# Restrict Objective-C++ runtime to Apple hosts. runtime_cpu.cpp and
# CpuKernels.cpp mirror the Metal entry points for CPU-only builds so no .mm
# files are compiled.
if(APPLE)
  target_sources(orchard_metal PRIVATE
    metal/runtime/runtime.mm
    metal/runtime/MetalKernels.mm
  )
else()
  target_sources(orchard_metal PRIVATE
    metal/runtime/runtime_cpu.cpp
    metal/runtime/CpuKernels.cpp
  )
endif()

target_include_directories(orchard_metal PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/metal)
target_link_libraries(orchard_metal PUBLIC orchard_core)
target_compile_definitions(orchard_metal PRIVATE ORCHARD_KERNEL_DIR="${CMAKE_CURRENT_SOURCE_DIR}/metal/kernels")
if(APPLE)
  target_link_libraries(orchard_metal PUBLIC Metal::Metal objc)
endif()

# Resolve cross-library references when only linking orchard_metal.

if(BUILD_TESTING)
  add_subdirectory(tests)
endif()

