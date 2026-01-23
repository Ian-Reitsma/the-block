add_executable(orchard_bench orchard_bench.cpp)
target_include_directories(orchard_bench PRIVATE ${CMAKE_SOURCE_DIR}/metal-tensor/metal)
target_link_libraries(orchard_bench PRIVATE orchard_core orchard_metal)
