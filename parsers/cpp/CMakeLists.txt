# SYNX — native C++ parser (parity with crates/synx-core).
#
# Build:
#   cmake -S parsers/cpp -B parsers/cpp/build -DSYNX_BUILD_TESTS=ON
#   cmake --build parsers/cpp/build
#   ctest --test-dir parsers/cpp/build
#
# Options:
#   SYNX_BUILD_TESTS=ON|OFF   build unit + conformance tests (default OFF)
#   SYNX_USE_ZLIB=AUTO|ON|OFF use zlib for .synxb DEFLATE (AUTO: detect; OFF: disable compile/decompile)
#   SYNX_NO_EXCEPTIONS=ON|OFF compile without exceptions / RTTI for UE5 (default ON)
#
# Integration with UE5 / Unreal Engine:
#   Drop parsers/cpp/include and parsers/cpp/src into your Module's source tree
#   and add "zlib" to PublicDependencyModuleNames (UE5 ships zlib). See examples/ue5.

cmake_minimum_required(VERSION 3.16)
project(synx-cpp
    VERSION 3.6.3
    DESCRIPTION "SYNX (Active Data Format) — native C++17 parser"
    LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT DEFINED SYNX_BUILD_TESTS)
    set(SYNX_BUILD_TESTS OFF CACHE BOOL "Build SYNX unit and conformance tests")
endif()
if(NOT DEFINED SYNX_USE_ZLIB)
    set(SYNX_USE_ZLIB AUTO CACHE STRING "Use zlib for .synxb DEFLATE (AUTO/ON/OFF)")
endif()
if(NOT DEFINED SYNX_NO_EXCEPTIONS)
    set(SYNX_NO_EXCEPTIONS ON CACHE BOOL "Compile without exceptions/RTTI (UE5-friendly)")
endif()

# ─── zlib detection ──────────────────────────────────────────────────────────
set(SYNX_HAVE_ZLIB OFF)
if(SYNX_USE_ZLIB STREQUAL "ON" OR SYNX_USE_ZLIB STREQUAL "AUTO")
    find_package(ZLIB)
    if(ZLIB_FOUND)
        set(SYNX_HAVE_ZLIB ON)
    elseif(SYNX_USE_ZLIB STREQUAL "ON")
        message(FATAL_ERROR "SYNX_USE_ZLIB=ON but zlib was not found")
    else()
        message(STATUS "SYNX: zlib not found; .synxb compile/decompile will return errors")
    endif()
endif()

# ─── library ────────────────────────────────────────────────────────────────
set(SYNX_SOURCES
    src/value.cpp
    src/parser.cpp
    src/engine.cpp
    src/engine_markers.cpp
    src/calc.cpp
    src/json.cpp
    src/stringify.cpp
    src/formatter.cpp
    src/diff.cpp
    src/binary.cpp
    src/synx.cpp)

add_library(synx STATIC ${SYNX_SOURCES})
add_library(synx::synx ALIAS synx)

target_include_directories(synx
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:include>)

if(SYNX_HAVE_ZLIB)
    target_link_libraries(synx PRIVATE ZLIB::ZLIB)
    target_compile_definitions(synx PRIVATE SYNX_HAVE_ZLIB=1)
endif()

if(SYNX_NO_EXCEPTIONS)
    if(MSVC)
        target_compile_options(synx PRIVATE /EHs-c- /GR-)
        target_compile_definitions(synx PRIVATE _HAS_EXCEPTIONS=0)
    else()
        target_compile_options(synx PRIVATE -fno-exceptions -fno-rtti)
    endif()
endif()

if(MSVC)
    target_compile_options(synx PRIVATE /W4 /permissive-)
else()
    target_compile_options(synx PRIVATE -Wall -Wextra -Wpedantic)
endif()

# ─── tests ──────────────────────────────────────────────────────────────────
if(SYNX_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

# ─── install ────────────────────────────────────────────────────────────────
include(GNUInstallDirs)
install(TARGETS synx
    EXPORT synxTargets
    ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR})
install(DIRECTORY include/ DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})
