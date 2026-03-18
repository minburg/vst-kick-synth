# CMakeLists.txt for building the Kick Synth Audio Unit v2 wrapper.
#
# This project uses free-audio/clap-wrapper to produce a self-contained
# AUv2 .component bundle that embeds the pre-built CLAP plugin binary.
#
# Required variables (pass via -D on the cmake command line):
#   CLAP_BINARY_LOCATION   - absolute path to the kick_synth.clap bundle directory
#   CLAP_WRAPPER_SOURCE_DIR - absolute path to a checkout of free-audio/clap-wrapper
#
# Optional:
#   PLUGIN_VERSION          - version string (major.minor.patch), defaults to "0.3.0"

cmake_minimum_required(VERSION 3.21)

# Validate / default the version before calling project() which requires semver
if(NOT DEFINED PLUGIN_VERSION OR NOT PLUGIN_VERSION MATCHES "^[0-9]+\\.[0-9]+")
    set(PLUGIN_VERSION "0.3.0")
endif()

project(kick-synth-auv2 VERSION ${PLUGIN_VERSION} LANGUAGES C CXX OBJC OBJCXX)

if(NOT APPLE)
    message(FATAL_ERROR "AUv2 is only supported on macOS")
endif()

if(NOT DEFINED CLAP_BINARY_LOCATION)
    message(FATAL_ERROR "CLAP_BINARY_LOCATION must be set to the path of the kick_synth.clap bundle directory")
endif()

if(NOT DEFINED CLAP_WRAPPER_SOURCE_DIR)
    message(FATAL_ERROR "CLAP_WRAPPER_SOURCE_DIR must be set to the path of a free-audio/clap-wrapper checkout")
endif()

# Let clap-wrapper download the CLAP SDK and AudioUnitSDK automatically via CPM
set(CLAP_WRAPPER_DOWNLOAD_DEPENDENCIES ON CACHE BOOL "" FORCE)

# Include clap-wrapper from the provided source directory.
# PROJECT_IS_TOP_LEVEL is FALSE here so clap-wrapper will not auto-create any
# default targets; we define our own AUv2 target below.
add_subdirectory("${CLAP_WRAPPER_SOURCE_DIR}" "${CMAKE_CURRENT_BINARY_DIR}/clap-wrapper" EXCLUDE_FROM_ALL)

# The AUv2 MODULE target
add_library(kick_synth_auv2 MODULE)

target_add_auv2_wrapper(
    TARGET      kick_synth_auv2
    OUTPUT_NAME "Kick Synth"

    BUNDLE_IDENTIFIER "com.convolution-dev.kick-synth"
    BUNDLE_VERSION    "${PROJECT_VERSION}"

    # AUv2 four-character codes for this instrument
    INSTRUMENT_TYPE   "aumu"          # software instrument (MIDI in, audio out)
    MANUFACTURER_NAME "Convolution DEV"
    MANUFACTURER_CODE "CnvD"         # must be exactly 4 characters
    SUBTYPE_CODE      "KkSy"         # must be exactly 4 characters, unique per plugin

    # Embed the CLAP bundle inside Contents/PlugIns so the AU is self-contained
    MACOS_EMBEDDED_CLAP_LOCATION "${CLAP_BINARY_LOCATION}"
)
