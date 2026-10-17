# Test runner — single executable, registers each test as a CTest entry.

add_executable(synx_tests
    test_main.cpp
    test_value.cpp
    test_parser.cpp
    test_calc.cpp
    test_json.cpp
    test_stringify.cpp
    test_diff.cpp
    test_binary.cpp
    test_engine.cpp
    test_engine_env.cpp
    test_multiline.cpp
    test_constraints.cpp
    test_conformance.cpp
    test_380.cpp)

target_link_libraries(synx_tests PRIVATE synx)
target_include_directories(synx_tests PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/../include
    ${CMAKE_CURRENT_SOURCE_DIR}/../src)

# Conformance corpus lives at the repo root.
target_compile_definitions(synx_tests PRIVATE
    SYNX_CONFORMANCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/../../../tests/conformance/cases")

# The binary tests check DEFLATE only when the library has it.
if(SYNX_HAVE_ZLIB)
    target_compile_definitions(synx_tests PRIVATE SYNX_HAVE_ZLIB=1)
endif()

# Run every registered test by name.
add_test(NAME synx_tests COMMAND synx_tests)

# Test sources carry UTF-8 literals (Cyrillic / CJK); tell MSVC to read them as such.
if(MSVC)
    target_compile_options(synx_tests PRIVATE /utf-8)
endif()
