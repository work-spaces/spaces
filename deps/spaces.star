"""
Build dependencies
"""

load("//@star/sdk/star/cmake.star", "cmake_add_configure_build_install")
load(
    "//@star/sdk/star/info.star",
    "info_is_platform_aarch64",
    "info_is_platform_linux",
    "info_is_platform_x86_64",
)
load("//@star/sdk/star/run.star", "run_add_exec")
load("//@star/sdk/star/ws.star", "workspace_get_absolute_path", "workspace_load_value")

if workspace_load_value("SPACES_DBUS_ENABLED"):
    musl_toolchain_file = []
    if info_is_platform_linux():
        musl_toolchain_file = [
            "-DCMAKE_TOOLCHAIN_FILE=" + workspace_get_absolute_path() + "/sysroot/share/cmake/musl-toolchain.cmake",
        ]

    cmake_add_configure_build_install(
        "libexpat",
        source_directory = "deps/libexpat/expat",
        configure_args = musl_toolchain_file + [
            "-GNinja",
            "-DEXPAT_BUILD_TOOLS:BOOL=OFF",
            "-DBUILD_SHARED_LIBS:BOOL=OFF",
            "-DEXPAT_BUILD_EXAMPLES:BOOL=OFF",
            "-DEXPAT_BUILD_DOCS:BOOL=OFF",
            "-DEXPAT_SHARED_LIBS:BOOL=OFF",
        ],
    )

    cmake_add_configure_build_install(
        "dbus",
        source_directory = "deps/dbus",
        configure_args = musl_toolchain_file + [
            "-GNinja",
            "-DBUILD_SHARED_LIBS:BOOL=OFF",
            "-DDBUS_BUILD_TESTS:BOOL=OFF",
            "-DDBUS_SESSION_SOCKET_DIR=/tmp",
            "-DEXPAT_INCLUDE_DIR:PATH=" + workspace_get_absolute_path() + "/build/install/include",
            "-DEXPAT_LIBRARY:PATH=" + workspace_get_absolute_path() + "/build/install/lib/libexpat.a",
        ],
        deps = [":libexpat"],
    )
