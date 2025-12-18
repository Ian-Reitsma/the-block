if(EXISTS "${PROJECT_SOURCE_DIR}/third_party/googletest/CMakeLists.txt")
  add_subdirectory("${PROJECT_SOURCE_DIR}/third_party/googletest" googletest)
  set(GTEST_LIB gtest_main)
elseif(FETCHCONTENT_FULLY_DISCONNECTED)
  find_package(GTest REQUIRED)
  set(GTEST_LIB GTest::gtest_main)
else()
  include(FetchContent)
  FetchContent_Declare(
    googletest
    URL https://github.com/google/googletest/archive/refs/tags/v1.14.0.zip
  )
  FetchContent_MakeAvailable(googletest)
  set(GTEST_LIB gtest_main)
endif()

add_executable(metal_tensor_tests tensor_tests.cpp multi_device_tests.cpp fallback_tests.cpp high_rank_tests.cpp)

# Link against orchard_metal; orchard_core is a transitive dependency.
target_link_libraries(metal_tensor_tests PRIVATE ${GTEST_LIB} orchard_metal)

target_compile_features(metal_tensor_tests PRIVATE cxx_std_20)

add_test(NAME metal_tensor_tests COMMAND metal_tensor_tests)
