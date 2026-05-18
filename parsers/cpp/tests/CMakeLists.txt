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
    test_conformance.cpp)

target_link_libraries(synx_tests PRIVATE synx)
target_include_directories(synx_tests PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/../include)

# Conformance corpus lives at the repo root.
target_compile_definitions(synx_tests PRIVATE
    SYNX_CONFORMANCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/../../../tests/conformance/cases")

# Run every registered test by name.
add_test(NAME synx_tests COMMAND synx_tests)
