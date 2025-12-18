cmake_minimum_required(VERSION 3.27)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")

if(APPLE)
  project(orchard LANGUAGES C CXX OBJCXX)
  find_package(Metal REQUIRED)
  # Do NOT force arm64e: pip/venv PyTorch wheels are typically arm64-only.
  # Allow users to override via -DCMAKE_OSX_ARCHITECTURES=... when configuring.
  if(NOT DEFINED CMAKE_OSX_ARCHITECTURES OR CMAKE_OSX_ARCHITECTURES STREQUAL "")
    set(CMAKE_OSX_ARCHITECTURES "arm64" CACHE STRING "Build architectures" FORCE)
  endif()
  set(CMAKE_OBJCXX_STANDARD 20)
  set(CMAKE_OBJCXX_STANDARD_REQUIRED ON)
  set(CMAKE_OBJCXX_EXTENSIONS ON)
  add_compile_options($<$<COMPILE_LANGUAGE:OBJCXX>:-fno-objc-arc>)
  add_link_options(-ObjC++)
else()
  project(orchard LANGUAGES C CXX)
endif()

option(ORCHARD_BUILD_EXPERIMENTAL "Build experimental PyTorch components" OFF)
option(FETCHCONTENT_FULLY_DISCONNECTED "Disable FetchContent network access" OFF)

include(CTest)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS ON)

add_compile_options(-std=gnu++20 -Wall -Wextra -pedantic)

add_subdirectory(metal-tensor)
add_subdirectory(benchmarks)
if(BUILD_TESTING AND TARGET metal_tensor_tests)
  add_custom_target(check
    COMMAND ${CMAKE_CTEST_COMMAND} --output-on-failure
    DEPENDS metal_tensor_tests
  )
endif()
if(APPLE AND ORCHARD_BUILD_EXPERIMENTAL)
  add_subdirectory(experimental/orchard_ops)
endif()
