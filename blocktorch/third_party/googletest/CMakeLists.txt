cmake_minimum_required(VERSION 3.13)
project(googletest LANGUAGES CXX)

add_library(gtest STATIC
  googletest/src/gtest-all.cc
)
target_include_directories(gtest PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/googletest/include
  ${CMAKE_CURRENT_SOURCE_DIR}/googletest
)

add_library(gtest_main STATIC googletest/src/gtest_main.cc)
target_link_libraries(gtest_main PUBLIC gtest)
target_include_directories(gtest_main PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/googletest/include
  ${CMAKE_CURRENT_SOURCE_DIR}/googletest
)
