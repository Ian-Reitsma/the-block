cmake_minimum_required(VERSION 3.27)
project(orchard_flash_attn_mps LANGUAGES C CXX OBJCXX)

find_package(Torch REQUIRED)
# Important: request the Interpreter too, and allow overriding Python3_EXECUTABLE
# so we can force the active venv Python.
find_package(Python3 COMPONENTS Interpreter Development REQUIRED)

# Discover torch's packaged lib directory (e.g. .../site-packages/torch/lib)
execute_process(
    COMMAND ${Python3_EXECUTABLE} -c "import os, torch; print(os.path.join(os.path.dirname(torch.__file__), 'lib'))"
    OUTPUT_VARIABLE TORCH_PYTHON_LIBDIR
    OUTPUT_STRIP_TRAILING_WHITESPACE
)
message(STATUS "TORCH_PYTHON_LIBDIR: ${TORCH_PYTHON_LIBDIR}")

# === Library sources ===
add_library(flash_attn SHARED
    mps/flash_attn.mm       # C++ PyTorch operator registration (main entry)
)

# === Include directories ===
target_include_directories(flash_attn PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/mps
    ${TORCH_INCLUDE_DIRS}
    ${Python3_INCLUDE_DIRS}
)

# === Link Torch, Python, and any other dependencies ===
target_link_libraries(flash_attn PRIVATE
    ${TORCH_LIBRARIES}
    ${Python3_LIBRARIES}
)

# === Find libtorch_python (for pybind11/type_caster support) ===
# On pip/conda installs, this typically lives in <site-packages>/torch/lib.
find_library(TORCH_PYTHON_LIBRARY
    NAMES torch_python
    PATHS
        ${TORCH_LIBRARY_DIRS}
        ${TORCH_PYTHON_LIBDIR}
)
if (NOT TORCH_PYTHON_LIBRARY)
    message(FATAL_ERROR "Could not find libtorch_python.dylib (torch_python) in TORCH_LIBRARY_DIRS=${TORCH_LIBRARY_DIRS} or TORCH_PYTHON_LIBDIR=${TORCH_PYTHON_LIBDIR}")
endif()
message(STATUS "Linking against torch_python: ${TORCH_PYTHON_LIBRARY}")
target_link_libraries(flash_attn PRIVATE ${TORCH_PYTHON_LIBRARY})

# === Apple frameworks needed for ObjC++ Metal code ===
# flash_attn.mm imports <Foundation/Foundation.h> and <Metal/Metal.h>, and uses
# Objective-C runtime symbols, so we must link the corresponding frameworks.
if(APPLE)
  find_library(FOUNDATION_FRAMEWORK Foundation REQUIRED)
  find_library(COREFOUNDATION_FRAMEWORK CoreFoundation REQUIRED)
  find_library(METAL_FRAMEWORK Metal REQUIRED)
  find_library(OBJC_LIBRARY objc REQUIRED)

  target_link_libraries(flash_attn PRIVATE
    ${FOUNDATION_FRAMEWORK}
    ${COREFOUNDATION_FRAMEWORK}
    ${METAL_FRAMEWORK}
    ${OBJC_LIBRARY}
  )
endif()


# === C++ and ObjC++ standard enforcement ===
target_compile_features(flash_attn PRIVATE cxx_std_20)
set_property(TARGET flash_attn PROPERTY OBJCXX_STANDARD 20)

# === Output settings ===
# Write the dylib into the repo-local kernel_lib folder (untracked) so
# orchard_bridge can load it from a stable, repo-relative path.
set_target_properties(flash_attn PROPERTIES
    PREFIX ""
    OUTPUT_NAME "libflash_attn"
    LIBRARY_OUTPUT_DIRECTORY ${CMAKE_CURRENT_LIST_DIR}/../kernel_lib/flashattn
)

# === Build output status messages (debugging) ===
message(STATUS "PYTHON EXECUTABLE: ${Python3_EXECUTABLE}")
message(STATUS "PYTHON INCLUDE DIRS: ${Python3_INCLUDE_DIRS}")
message(STATUS "TORCH LIB DIRS: ${TORCH_LIBRARY_DIRS}")
