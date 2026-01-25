add_executable(metal_tensor_tests
  harness.cpp
  tensor_tests.cpp
  multi_device_tests.cpp
  fallback_tests.cpp
  high_rank_tests.cpp
)

# Link against orchard_metal; orchard_core is a transitive dependency.
target_link_libraries(metal_tensor_tests PRIVATE orchard_metal)

target_compile_features(metal_tensor_tests PRIVATE cxx_std_20)
target_include_directories(metal_tensor_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})

add_test(NAME metal_tensor_tests COMMAND metal_tensor_tests)
