cmake_minimum_required(VERSION 3.16)
project(synx_cpp_examples VERSION 3.6.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(synx_cpp_headers INTERFACE)
target_include_directories(synx_cpp_headers INTERFACE
    "${CMAKE_CURRENT_SOURCE_DIR}/include"
    "${CMAKE_CURRENT_SOURCE_DIR}/../c-header/include"
)

if(NOT SYNX_C_LIBRARY)
    message(WARNING
        "SYNX_C_LIBRARY not set. Example target skipped. "
        "Build native lib: cargo build -p synx-c --release\n"
        "Configure with: cmake -B build -DSYNX_C_LIBRARY=/absolute/path/to/libsynx_c")
else()
    add_executable(synx_cpp_minimal examples/minimal.cpp)
    target_link_libraries(synx_cpp_minimal PRIVATE synx_cpp_headers "${SYNX_C_LIBRARY}")
endif()
