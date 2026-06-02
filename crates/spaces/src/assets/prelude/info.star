"""
Spaces info built-ins
"""

def info_set_max_queue_count(max_queue_count: int):
    """
    Set the maximum number of jobs that can be queued.

    Args:
        max_queue_count: The maximum number of jobs that can be queued
    """
    info.set_max_queue_count(max_queue_count)

def info_set_minimum_version(version: str):
    """
    Set the minimum version of Spaces required to run the workflow

    Args:
        version: The minimum version of Spaces required to run the workflow
    """
    info.set_minimum_version(version)

def info_get_cpu_count() -> int:
    """
    Get the number of CPUs available

    Returns:
        The number of CPUs available
    """
    return info.get_cpu_count()

def info_get_path_to_store() -> str:
    """
    Gets the path to the spaces store

    Returns:
        The path to the store
    """
    return info.get_path_to_store()

def info_is_platform_x86_64() -> bool:
    """
    Check if the platform is x86_64

    Returns:
        True if the platform is x86_64, False otherwise
    """
    return info.is_platform_x86_64()

def info_is_platform_aarch64() -> bool:
    """
    Check if the platform is aarch64

    Returns:
        True if the platform is aarch64, False otherwise
    """
    return info.is_platform_aarch64()

def info_is_platform_linux() -> bool:
    """
    Check if the platform is Linux

    Returns:
        True if the platform is Linux, False otherwise
    """
    return info.is_platform_linux()

def info_is_platform_macos() -> bool:
    """
    Check if the platform is macOS

    Returns:
        True if the platform is macOS, False otherwise
    """
    return info.is_platform_macos()

def info_is_platform_windows() -> bool:
    """
    Check if the platform is Windows

    Returns:
        True if the platform is Windows, False otherwise
    """
    return info.is_platform_windows()

def info_get_platform_name() -> str:
    """
    Get the platform name

    Returns:
        The platform name
    """
    return info.get_platform_name()

def info_get_supported_platforms() -> list[str]:
    """
    Get the supported platforms

    Returns:
        The supported platforms
    """
    return info.get_supported_platforms()

def info_get_path_to_spaces_tools() -> str:
    """
    Get the path to the Spaces tools folder

    Returns:
        The path to the Spaces tools folder
    """
    return info.get_path_to_spaces_tools()

def info_parse_log_file(path: str) -> dict:
    """
    Parses a log file

    Args:
        path: path to the log file

    Returns:
        dict with members `header` and lines
    """
    return info.parse_log_file(path)

def info_set_required_semver(required: str):
    """
    Set the required `spaces` semver for the workflow

    Args:
        required: The required semver for the workflow
    """
    info.set_required_semver(required)

def info_is_ci() -> bool:
    """
    Check if the workflow is running in a CI environment.

    Returns:
        True if `--ci` is passed when running `spaces`, False otherwise
    """
    return info.is_ci()

def info_get_execution_phase() -> str:
    """
    Get the execution phase

    Returns:
        The execution phase
    """
    info_set_minimum_version("0.15.23")
    return info.get_execution_phase()

def info_is_execution_phase_inspect() -> bool:
    """
    Check if the execution phase is inspect

    Returns:
        True if the execution phase is inspect, False otherwise
    """
    return info_get_execution_phase() == "Inspect"

def info_is_execution_phase_run() -> bool:
    """
    Check if the execution phase is run

    Returns:
        True if the execution phase is run, False otherwise
    """
    return info_get_execution_phase() == "Run"

def info_is_execution_phase_checkout() -> bool:
    """
    Check if the execution phase is checkout

    Returns:
        True if the execution phase is checkout, False otherwise
    """
    return info_get_execution_phase() == "Checkout"
