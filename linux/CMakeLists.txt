cmake_minimum_required(VERSION 3.10)
set(PROJECT_NAME "kservice_printer")
project(${PROJECT_NAME} LANGUAGES CXX)

include("${CMAKE_CURRENT_SOURCE_DIR}/../cargokit/cmake/cargokit.cmake")
apply_cargokit(${PROJECT_NAME} "../rust_printer_core" "kservice_printer_core" "frb_init")

set(kservice_printer_bundled_libraries
  ${${PROJECT_NAME}_cargokit_lib}
  PARENT_SCOPE
)
