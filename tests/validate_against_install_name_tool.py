#!/usr/bin/env python3
"""
Validation script for macho-tool against install_name_tool.

This script finds real-world dylib files, performs various modifications using both
our macho-tool and Apple's install_name_tool, then compares the results to ensure
our implementation is correct.
"""

import os
import sys
import subprocess
import tempfile
import shutil
import random
from pathlib import Path
from typing import List, Tuple, Optional
import argparse


class Colors:
    """ANSI color codes for terminal output."""
    GREEN = '\033[92m'
    RED = '\033[91m'
    YELLOW = '\033[93m'
    BLUE = '\033[94m'
    CYAN = '\033[96m'
    RESET = '\033[0m'
    BOLD = '\033[1m'


def is_fat_binary(dylib_path: Path) -> bool:
    """Check if a file is a fat/universal binary."""
    try:
        result = subprocess.run(
            ['file', str(dylib_path)],
            capture_output=True,
            text=True,
            check=True
        )
        return 'universal binary' in result.stdout.lower() or 'fat file' in result.stdout.lower()
    except subprocess.CalledProcessError:
        return False


def find_dylibs(search_dir: Path, limit: int = 100) -> List[Path]:
    """Find all .dylib files in the given directory, excluding fat binaries and symlinks."""
    print(f"{Colors.BLUE}Searching for dylib files in {search_dir}...{Colors.RESET}")

    dylibs = []
    skipped_symlinks = 0

    for root, dirs, files in os.walk(search_dir):
        for file in files:
            if file.endswith('.dylib'):
                path = Path(root) / file

                # Skip symlinks (they may point to non-existent files)
                if path.is_symlink():
                    skipped_symlinks += 1
                    continue

                # Also check if the file actually exists (not a broken symlink)
                if not path.exists():
                    skipped_symlinks += 1
                    continue

                dylibs.append(path)
                if len(dylibs) >= limit:
                    break
        if len(dylibs) >= limit:
            break

    print(f"{Colors.GREEN}Found {len(dylibs)} dylib files (skipped {skipped_symlinks} symlinks){Colors.RESET}")
    return dylibs


def get_install_names(dylib_path: Path) -> Tuple[Optional[str], List[str], List[str]]:
    """Get install name, rpaths, and dependencies using otool."""
    try:
        # Get install name
        result = subprocess.run(
            ['otool', '-D', str(dylib_path)],
            capture_output=True,
            text=True,
            check=True
        )
        lines = result.stdout.strip().split('\n')
        install_name = lines[1] if len(lines) > 1 else None

        # Get rpaths
        rpaths = []
        result = subprocess.run(
            ['otool', '-l', str(dylib_path)],
            capture_output=True,
            text=True,
            check=True
        )
        lines = result.stdout.split('\n')
        i = 0
        while i < len(lines):
            if 'cmd LC_RPATH' in lines[i]:
                # Next few lines contain the path
                for j in range(i + 1, min(i + 5, len(lines))):
                    if 'path ' in lines[j]:
                        path = lines[j].split('path ')[1].split(' (')[0].strip()
                        rpaths.append(path)
                        break
            i += 1

        # Get dependencies
        result = subprocess.run(
            ['otool', '-L', str(dylib_path)],
            capture_output=True,
            text=True,
            check=True
        )
        lines = result.stdout.strip().split('\n')
        dependencies = []
        for line in lines:
            line = line.strip()
            # Skip header lines (filename or architecture headers)
            if line.endswith(':') or not line:
                continue
            # Extract dependency (format: "path (compatibility version ...)")
            if ' (' in line:
                dep = line.split(' (')[0].strip()
                dependencies.append(dep)

        return install_name, rpaths, dependencies

    except subprocess.CalledProcessError as e:
        print(f"{Colors.RED}Error running otool: {e}{Colors.RESET}")
        return None, [], []


def run_install_name_tool(dylib_path: Path, *args) -> bool:
    """Run install_name_tool with given arguments."""
    try:
        subprocess.run(
            ['install_name_tool'] + list(args) + [str(dylib_path)],
            capture_output=True,
            check=True
        )
        return True
    except subprocess.CalledProcessError as e:
        stderr = e.stderr.decode() if e.stderr else ''
        # Some operations are expected to fail (e.g., deleting non-existent rpath)
        if 'no such path' in stderr.lower() or 'not found' in stderr.lower():
            return False
        print(f"{Colors.YELLOW}install_name_tool warning: {stderr}{Colors.RESET}")
        return False


def run_macho_tool(macho_tool_path: Path, dylib_path: Path, *args) -> bool:
    """Run our macho-tool with given arguments."""
    try:
        result = subprocess.run(
            [str(macho_tool_path)] + list(args) + [str(dylib_path)],
            capture_output=True,
            text=True,
            check=True
        )
        return True
    except subprocess.CalledProcessError as e:
        print(f"{Colors.RED}macho-tool error: {e.stderr}{Colors.RESET}")
        return False


def compare_dylibs(path1: Path, path2: Path, save_dir: Path = None, test_name: str = "") -> Tuple[bool, str]:
    """
    Compare two dylib files to see if they have the same install names and rpaths.
    Returns (success, message).
    If save_dir is provided and comparison fails, save the files for later analysis.
    """
    id1, rpaths1, deps1 = get_install_names(path1)
    id2, rpaths2, deps2 = get_install_names(path2)

    differences = []

    # Compare install names
    if id1 != id2:
        differences.append(f"Install name differs: '{id1}' vs '{id2}'")

    # Compare rpaths (order may differ, so compare as sets)
    if set(rpaths1) != set(rpaths2):
        differences.append(f"RPaths differ: {rpaths1} vs {rpaths2}")

    # Compare dependencies (order matters for dependencies)
    if deps1 != deps2:
        differences.append(f"Dependencies differ: {deps1} vs {deps2}")

    if differences:
        if save_dir:
            save_dir.mkdir(parents=True, exist_ok=True)
            shutil.copy(path1, save_dir / f"{test_name}_apple.dylib")
            shutil.copy(path2, save_dir / f"{test_name}_ours.dylib")
        return False, '\n  '.join(differences)

    # Check binary equality as a final check
    with open(path1, 'rb') as f1, open(path2, 'rb') as f2:
        data1 = f1.read()
        data2 = f2.read()

        if len(data1) != len(data2):
            if save_dir:
                save_dir.mkdir(parents=True, exist_ok=True)
                shutil.copy(path1, save_dir / f"{test_name}_apple.dylib")
                shutil.copy(path2, save_dir / f"{test_name}_ours.dylib")
            return False, f"Binary sizes differ : {len(data1)} vs {len(data2)}"

        if data1 != data2:
            if save_dir:
                save_dir.mkdir(parents=True, exist_ok=True)
                shutil.copy(path1, save_dir / f"{test_name}_apple.dylib")
                shutil.copy(path2, save_dir / f"{test_name}_ours.dylib")
            return False, "Binary contents differ"

    return True, "All fields match"


class TestCase:
    """Represents a single test case."""

    def __init__(self, name: str, description: str):
        self.name = name
        self.description = description
        self.passed = 0
        self.failed = 0
        self.skipped = 0
        self.skip_reasons = []  # Track reasons for skips
        self.failed_tests = []  # Store information about failed tests
        self.failed_dylibs = []  # Track names of dylibs that failed

    def run(self, dylib: Path, macho_tool: Path, temp_dir: Path) -> bool:
        """Run the test case. To be overridden by subclasses."""
        raise NotImplementedError

    def log_failure(self, dylib_name: str, install_cmd: list, macho_cmd: list, error_msg: str):
        """Log detailed information about a failed test."""
        self.failed_tests.append({
            'dylib': dylib_name,
            'install_name_tool_cmd': ' '.join(['install_name_tool'] + install_cmd),
            'macho_tool_cmd': ' '.join([str(macho_cmd[0])] + macho_cmd[1:]),
            'error': error_msg
        })


class AddRpathTest(TestCase):
    """Test adding an rpath."""

    def __init__(self):
        super().__init__("add_rpath", "Add a new rpath to the dylib")

    def run(self, dylib: Path, macho_tool: Path, temp_dir: Path) -> bool:
        # Create two copies
        copy_install = temp_dir / "install_name_tool_copy.dylib"
        copy_macho = temp_dir / "macho_tool_copy.dylib"

        try:
            shutil.copy(dylib, copy_install)
            shutil.copy(dylib, copy_macho)
        except (OSError, IOError) as e:
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: Failed to copy file ({e})")
            return True

        # Add a unique rpath
        new_rpath = f"@loader_path/test_{random.randint(1000, 9999)}"

        # Check if rpath already exists
        _, existing_rpaths, _ = get_install_names(dylib)
        if new_rpath in existing_rpaths:
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: RPATH already exists")
            return True

        # Apply with install_name_tool
        if not run_install_name_tool(copy_install, '-add_rpath', new_rpath):
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: install_name_tool failed (likely no slack space)")
            return True

        # Apply with macho-tool
        if not run_macho_tool(macho_tool, copy_macho, '-add_rpath', new_rpath):
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            return False

        # Compare results
        success, msg = compare_dylibs(copy_install, copy_macho)
        if success:
            self.passed += 1
        else:
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            print(f"{Colors.RED}  {msg}{Colors.RESET}")

        return success


class DeleteRpathTest(TestCase):
    """Test deleting an existing rpath."""

    def __init__(self):
        super().__init__("delete_rpath", "Delete an existing rpath from the dylib")

    def run(self, dylib: Path, macho_tool: Path, temp_dir: Path) -> bool:
        # Get existing rpaths
        _, rpaths, _ = get_install_names(dylib)

        if not rpaths:
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: No RPATHs to delete")
            return True

        # Pick a random rpath to delete
        rpath_to_delete = random.choice(rpaths)

        # Create two copies
        copy_install = temp_dir / "install_name_tool_copy.dylib"
        copy_macho = temp_dir / "macho_tool_copy.dylib"

        try:
            shutil.copy(dylib, copy_install)
            shutil.copy(dylib, copy_macho)
        except (OSError, IOError) as e:
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: Failed to copy file ({e})")
            return True

        # Apply with install_name_tool
        if not run_install_name_tool(copy_install, '-delete_rpath', rpath_to_delete):
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: install_name_tool delete failed")
            return True

        # Apply with macho-tool
        if not run_macho_tool(macho_tool, copy_macho, '-delete_rpath', rpath_to_delete):
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            return False

        # Compare results
        success, msg = compare_dylibs(copy_install, copy_macho)
        if success:
            self.passed += 1
        else:
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            print(f"{Colors.RED}  {msg}{Colors.RESET}")

        return success


class ChangeRpathTest(TestCase):
    """Test changing an existing rpath."""

    def __init__(self):
        super().__init__("change_rpath", "Change an existing rpath")

    def run(self, dylib: Path, macho_tool: Path, temp_dir: Path) -> bool:
        # Get existing rpaths
        _, rpaths, _ = get_install_names(dylib)

        if not rpaths:
            self.skipped += 1
            return True

        # Pick a random rpath to change
        old_rpath = random.choice(rpaths)
        new_rpath = f"@loader_path/changed_{random.randint(1000, 9999)}"

        # Create two copies
        copy_install = temp_dir / "install_name_tool_copy.dylib"
        copy_macho = temp_dir / "macho_tool_copy.dylib"

        try:
            shutil.copy(dylib, copy_install)
            shutil.copy(dylib, copy_macho)
        except (OSError, IOError) as e:
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: Failed to copy file ({e})")
            return True

        # Apply with install_name_tool
        if not run_install_name_tool(copy_install, '-rpath', old_rpath, new_rpath):
            self.skipped += 1
            return True

        # Apply with macho-tool
        if not run_macho_tool(macho_tool, copy_macho, '-rpath', old_rpath, new_rpath):
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            return False

        # Compare results
        success, msg = compare_dylibs(copy_install, copy_macho)
        if success:
            self.passed += 1
        else:
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            print(f"{Colors.RED}  {msg}{Colors.RESET}")

        return success


class ChangeIdTest(TestCase):
    """Test changing the dylib ID."""

    def __init__(self):
        super().__init__("change_id", "Change the dylib install name (ID)")

    def run(self, dylib: Path, macho_tool: Path, temp_dir: Path) -> bool:
        # Get existing install name
        install_name, _, _ = get_install_names(dylib)

        if not install_name:
            self.skipped += 1
            return True

        # Create a new install name
        new_id = f"@rpath/modified_{random.randint(1000, 9999)}.dylib"

        # Create two copies
        copy_install = temp_dir / "install_name_tool_copy.dylib"
        copy_macho = temp_dir / "macho_tool_copy.dylib"

        try:
            shutil.copy(dylib, copy_install)
            shutil.copy(dylib, copy_macho)
        except (OSError, IOError) as e:
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: Failed to copy file ({e})")
            return True

        # Apply with install_name_tool
        install_cmd = ['-id', new_id, str(copy_install)]
        if not run_install_name_tool(copy_install, '-id', new_id):
            self.skipped += 1
            return True

        # Apply with macho-tool
        macho_cmd = [macho_tool, '-id', new_id, str(copy_macho)]
        if not run_macho_tool(macho_tool, copy_macho, '-id', new_id):
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            self.log_failure(dylib.name, install_cmd, macho_cmd, "macho-tool failed to run")
            return False

        # Compare results
        save_dir = Path('testfiles/failures') if temp_dir.name else None
        success, msg = compare_dylibs(copy_install, copy_macho, save_dir, f"{dylib.stem}_change_id")
        if success:
            self.passed += 1
        else:
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            self.log_failure(dylib.name, install_cmd, macho_cmd, msg)
            print(f"{Colors.RED}  {msg}{Colors.RESET}")
            print(f"{Colors.YELLOW}  install_name_tool: {' '.join(install_cmd)}{Colors.RESET}")
            print(f"{Colors.YELLOW}  macho-tool: {' '.join([str(x) for x in macho_cmd])}{Colors.RESET}")
            if save_dir:
                print(f"{Colors.CYAN}  Saved failing binaries to {save_dir}/{dylib.stem}_change_id_*.dylib{Colors.RESET}")

        return success


class ChangeDependencyTest(TestCase):
    """Test changing a dependency."""

    def __init__(self):
        super().__init__("change_dependency", "Change a dylib dependency")

    def run(self, dylib: Path, macho_tool: Path, temp_dir: Path) -> bool:
        # Get existing dependencies
        _, _, deps = get_install_names(dylib)

        if len(deps) < 2:  # Need at least 2 (one is usually the dylib itself)
            self.skipped += 1
            return True

        # Pick a random dependency to change (skip the first one which is often self)
        old_dep = random.choice(deps[1:])
        new_dep = f"@rpath/changed_dep_{random.randint(1000, 9999)}.dylib"

        # Create two copies
        copy_install = temp_dir / "install_name_tool_copy.dylib"
        copy_macho = temp_dir / "macho_tool_copy.dylib"

        try:
            shutil.copy(dylib, copy_install)
            shutil.copy(dylib, copy_macho)
        except (OSError, IOError) as e:
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: Failed to copy file ({e})")
            return True

        # Apply with install_name_tool
        if not run_install_name_tool(copy_install, '-change', old_dep, new_dep):
            self.skipped += 1
            return True

        # Apply with macho-tool
        if not run_macho_tool(macho_tool, copy_macho, '-change', old_dep, new_dep):
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            return False

        # Compare results
        success, msg = compare_dylibs(copy_install, copy_macho)
        if success:
            self.passed += 1
        else:
            self.failed += 1
            self.failed_dylibs.append(dylib.name)
            print(f"{Colors.RED}  {msg}{Colors.RESET}")

        return success


class MultipleOperationsTest(TestCase):
    """Test multiple operations at once."""

    def __init__(self):
        super().__init__("multiple_ops", "Perform multiple operations in one command")

    def run(self, dylib: Path, macho_tool: Path, temp_dir: Path) -> bool:
        # Get current state
        install_name, rpaths, _ = get_install_names(dylib)

        # Build operation lists
        install_args = []
        macho_args = []

        # Add an rpath
        new_rpath = f"@loader_path/multi_{random.randint(1000, 9999)}"
        install_args.extend(['-add_rpath', new_rpath])
        macho_args.extend(['-add_rpath', new_rpath])

        # If there's an existing rpath, delete it
        if rpaths:
            rpath_to_delete = rpaths[0]
            install_args.extend(['-delete_rpath', rpath_to_delete])
            macho_args.extend(['-delete_rpath', rpath_to_delete])

        # If there's an install name, change it
        if install_name:
            new_id = f"@rpath/multi_{random.randint(1000, 9999)}.dylib"
            install_args.extend(['-id', new_id])
            macho_args.extend(['-id', new_id])

        if len(install_args) < 4:  # Need at least 2 operations
            self.skipped += 1
            return True

        # Create two copies
        copy_install = temp_dir / "install_name_tool_copy.dylib"
        copy_macho = temp_dir / "macho_tool_copy.dylib"

        try:
            shutil.copy(dylib, copy_install)
            shutil.copy(dylib, copy_macho)
        except (OSError, IOError) as e:
            self.skipped += 1
            self.skip_reasons.append(f"{dylib.name}: Failed to copy file ({e})")
            return True

        # Apply with install_name_tool
        if not run_install_name_tool(copy_install, *install_args):
            self.skipped += 1
            return True

        # Apply with macho-tool
        if not run_macho_tool(macho_tool, copy_macho, *macho_args):
            self.failed += 1
            return False

        # Compare results
        success, msg = compare_dylibs(copy_install, copy_macho)
        if success:
            self.passed += 1
        else:
            self.failed += 1
            print(f"{Colors.RED}  {msg}{Colors.RESET}")

        return success


def main():
    parser = argparse.ArgumentParser(description='Validate macho-tool against install_name_tool')
    parser.add_argument(
        '--search-dir',
        type=Path,
        default=Path('/Users/wolfv/Library/Caches/rattler/cache'),
        help='Directory to search for dylib files'
    )
    parser.add_argument(
        '--macho-tool',
        type=Path,
        default=Path('target/debug/examples/macho-tool'),
        help='Path to macho-tool binary'
    )
    parser.add_argument(
        '--num-dylibs',
        type=int,
        default=20,
        help='Number of random dylibs to test'
    )
    parser.add_argument(
        '--verbose',
        action='store_true',
        help='Verbose output'
    )

    args = parser.parse_args()

    # Check if macho-tool exists
    if not args.macho_tool.exists():
        print(f"{Colors.RED}Error: macho-tool not found at {args.macho_tool}{Colors.RESET}")
        print(f"{Colors.YELLOW}Build it with: cargo build --example macho-tool --features build,read,write{Colors.RESET}")
        return 1

    # Check if install_name_tool exists
    try:
        result = subprocess.run(['which', 'install_name_tool'], capture_output=True, text=True)
        if result.returncode != 0:
            print(f"{Colors.RED}Error: install_name_tool not found. This test requires macOS.{Colors.RESET}")
            return 1
    except FileNotFoundError:
        print(f"{Colors.RED}Error: install_name_tool not found. This test requires macOS.{Colors.RESET}")
        return 1

    # Find dylibs
    dylibs = find_dylibs(args.search_dir, limit=1000)
    if not dylibs:
        print(f"{Colors.RED}No dylib files found in {args.search_dir}{Colors.RESET}")
        return 1

    # Select random subset
    random.shuffle(dylibs)
    selected_dylibs = dylibs[:args.num_dylibs]

    print(f"\n{Colors.BOLD}Testing {len(selected_dylibs)} random dylibs{Colors.RESET}\n")

    # Define test cases
    test_cases = [
        AddRpathTest(),
        DeleteRpathTest(),
        ChangeRpathTest(),
        ChangeIdTest(),
        ChangeDependencyTest(),
        MultipleOperationsTest(),
    ]

    # Run tests
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)

        for i, dylib in enumerate(selected_dylibs, 1):
            print(f"{Colors.BOLD}[{i}/{len(selected_dylibs)}] Testing: {dylib.name}{Colors.RESET}")

            if args.verbose:
                id, rpaths, deps = get_install_names(dylib)
                print(f"  ID: {id}")
                print(f"  RPaths: {len(rpaths)}, Deps: {len(deps)}")

            for test_case in test_cases:
                test_temp = temp_path / f"test_{i}_{test_case.name}"
                test_temp.mkdir(exist_ok=True)

                try:
                    test_case.run(dylib, args.macho_tool, test_temp)
                except Exception as e:
                    print(f"{Colors.RED}  {test_case.name}: Exception: {e}{Colors.RESET}")
                    test_case.failed += 1

                # Clean up temp files
                shutil.rmtree(test_temp, ignore_errors=True)

    # Print results
    print(f"\n{Colors.BOLD}{'='*60}{Colors.RESET}")
    print(f"{Colors.BOLD}Test Results:{Colors.RESET}\n")

    total_passed = 0
    total_failed = 0
    total_skipped = 0

    for test_case in test_cases:
        total_passed += test_case.passed
        total_failed += test_case.failed
        total_skipped += test_case.skipped

        status_color = Colors.GREEN if test_case.failed == 0 else Colors.RED

        print(f"{Colors.BOLD}{test_case.name}:{Colors.RESET} {test_case.description}")
        print(f"  {status_color}Passed: {test_case.passed}{Colors.RESET}")
        if test_case.failed > 0:
            print(f"  {Colors.RED}Failed: {test_case.failed}{Colors.RESET}")
        if test_case.skipped > 0:
            print(f"  {Colors.YELLOW}Skipped: {test_case.skipped}{Colors.RESET}")
            # Show first few skip reasons as examples
            if test_case.skip_reasons:
                print(f"  {Colors.YELLOW}Skip reasons (first 3):{Colors.RESET}")
                for reason in test_case.skip_reasons[:3]:
                    print(f"    - {reason}")
                if len(test_case.skip_reasons) > 3:
                    print(f"    ... and {len(test_case.skip_reasons) - 3} more")
        print()

    print(f"{Colors.BOLD}{'='*60}{Colors.RESET}")
    print(f"{Colors.BOLD}Overall:{Colors.RESET}")
    print(f"  {Colors.GREEN}Passed:  {total_passed}{Colors.RESET}")
    print(f"  {Colors.RED}Failed:  {total_failed}{Colors.RESET}")
    print(f"  {Colors.YELLOW}Skipped: {total_skipped}{Colors.RESET}")

    if total_failed == 0:
        print(f"\n{Colors.GREEN}{Colors.BOLD}✓ All tests passed!{Colors.RESET}")
        return 0
    else:
        print(f"\n{Colors.RED}{Colors.BOLD}✗ Some tests failed{Colors.RESET}")
        return 1


if __name__ == '__main__':
    sys.exit(main())
