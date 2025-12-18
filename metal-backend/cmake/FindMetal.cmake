# FindMetal.cmake - locate Metal frameworks and configure macOS SDK

if(NOT APPLE)
    add_library(Metal::Metal INTERFACE IMPORTED)
    set(Metal_FOUND TRUE)
    return()
endif()

find_program(METAL_XCRUN xcrun REQUIRED)

# Determine SDK path
execute_process(
    COMMAND ${METAL_XCRUN} --sdk macosx --show-sdk-path
    OUTPUT_VARIABLE METAL_SDK_PATH
    OUTPUT_STRIP_TRAILING_WHITESPACE
    RESULT_VARIABLE _sdk_path_result
)
if(NOT _sdk_path_result EQUAL 0 OR METAL_SDK_PATH STREQUAL "")
    message(FATAL_ERROR "Failed to locate macOS SDK. Install Xcode command line tools.")
endif()
set(CMAKE_OSX_SYSROOT "${METAL_SDK_PATH}" CACHE PATH "macOS SDK" FORCE)

# Determine SDK version and derive deployment target
execute_process(
    COMMAND ${METAL_XCRUN} --sdk macosx --show-sdk-version
    OUTPUT_VARIABLE METAL_SDK_VERSION
    OUTPUT_STRIP_TRAILING_WHITESPACE
    RESULT_VARIABLE _sdk_ver_result
)
if(NOT _sdk_ver_result EQUAL 0)
    message(FATAL_ERROR "Failed to determine macOS SDK version.")
endif()
string(REGEX MATCH "^[0-9]+\.[0-9]+" METAL_DEPLOYMENT_TARGET "${METAL_SDK_VERSION}")
set(CMAKE_OSX_DEPLOYMENT_TARGET "${METAL_DEPLOYMENT_TARGET}" CACHE STRING "macOS deployment target" FORCE)

# Locate required frameworks
find_library(Metal_FRAMEWORK Metal REQUIRED)
find_library(MetalKit_FRAMEWORK MetalKit REQUIRED)
find_library(Foundation_FRAMEWORK Foundation REQUIRED)

set(Metal_LIBRARIES ${Metal_FRAMEWORK} ${MetalKit_FRAMEWORK} ${Foundation_FRAMEWORK})
add_library(Metal::Metal INTERFACE IMPORTED)
set_target_properties(Metal::Metal PROPERTIES
    INTERFACE_LINK_LIBRARIES "${Metal_LIBRARIES}"
)

set(Metal_FOUND TRUE)

