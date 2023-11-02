#!/usr/bin/env python3

import argparse
import multiprocessing
import os
import os.path
import platform
import subprocess
import sys
import time
from subprocess import PIPE


class TestRunner:
    """ TestRunner class. Provides methods for running the test and validating test success.
        Subclassed by QemuTestRunner and UhyveTestRunner that extend this class
    """

    def __init__(self,
                 test_command,
                 timeout_seconds: int,
                 num_cores=1,
                 memory_in_megabyte=512,
                 gdb_enabled=False,
                 verbose=False):
        online_cpus = multiprocessing.cpu_count()
        if num_cores > online_cpus:
            print("WARNING: You specified num_cores={}, however only {} cpu cores are available."
                  " Setting num_cores to {}".format(num_cores, online_cpus, online_cpus), file=sys.stderr)
            num_cores = online_cpus
        self.num_cores: int = num_cores
        self.memory_MB: int = memory_in_megabyte
        self.gdb_enabled: bool = gdb_enabled
        self.gdb_port = None
        self.verbose: bool = verbose
        self.test_command = test_command
        self.custom_env = None
        self.timeout: int = timeout_seconds

    def validate_test_success(self, rc, stdout, stderr, execution_time) -> bool:
        """

        :param rc: TestRunner ignores rc, but subclasses should evaluate the rc
        :param stdout:
        :param stderr: ToDo: Not sure if we actually need this, does hermit use this?
        :param execution_time:
        :return: bool - true indicates success
        """
        # ToDo: possibly add test failure due to excessive execution time?
        #       This could be done if a test suddenly regresses compared to usual execution time
        #       Probably need criterion + stable execution environment for this
        if not validate_stdout(stdout):
            print("Test failed due to Panic. Dumping output (stderr):\n{}\n\n"
                  "Dumping stdout:\n{}\nFinished Dump".format(stderr, stdout), file=sys.stderr)
            return False
        else:
            return True

    def run_test(self):
        """
        :return: returncode, stdout, stderr, elapsed_time, timed_out: bool
        """
        print("Calling {}".format(type(self).__name__))
        try:
            start_time = time.perf_counter()  # https://docs.python.org/3/library/time.html#time.perf_counter
            if self.custom_env is None:
                p = subprocess.run(self.test_command, stdout=PIPE, stderr=PIPE, universal_newlines=True,
                        timeout=self.timeout)
            else:
                p = subprocess.run(self.test_command, stdout=PIPE, stderr=PIPE, universal_newlines=True,
                                timeout=self.timeout, env=self.custom_env)
            end_time = time.perf_counter()
            elapsed_time = end_time - start_time
        except subprocess.TimeoutExpired as e:
            elapsed_time = self.timeout * (10 ** 9)
            return None, e.stdout, e.stderr, elapsed_time, True

        # ToDo: add some timeout
        return p.returncode, p.stdout, p.stderr, elapsed_time, False


class QemuTestRunner(TestRunner):
    """
    Test Runner for QEMU. Requires a path to the bootloader and the test_exe. 
    Verbose is not an option, so the '-vv' flag for `hermit_test_runner.py` behaves the same as `-v`.
    """

    def __init__(self,
                 test_exe_path: str,
                 timeout_seconds: int,
                 bootloader_path: str = '../loader/target/x86_64/debug/hermit-loader',
                 num_cores=1,
                 memory_in_megabyte=512,
                 gdb_enabled=False):
        assert os.path.isfile(test_exe_path), "Invalid path to test executable: {}".format(test_exe_path)
        assert os.path.isfile(bootloader_path), "Invalid bootloader path: {}".format(bootloader_path)
        self.bootloader_path = os.path.abspath(bootloader_path)
        test_command = ['qemu-system-x86_64',
                        '-display', 'none',
                        '-smp', str(num_cores),
                        '-m', str(memory_in_megabyte) + 'M',
                        '-serial', 'stdio',
                        '-kernel', bootloader_path,
                        '-initrd', test_exe_path,
                        '-cpu', 'qemu64,apic,fsgsbase,rdtscp,xsave,xsaveopt,fxsr',
                        '-device', 'isa-debug-exit,iobase=0xf4,iosize=0x04',
                        ]
        super().__init__(test_command, 
                        timeout_seconds=timeout_seconds, 
                        num_cores = num_cores, 
                        memory_in_megabyte = memory_in_megabyte, 
                        gdb_enabled = gdb_enabled, 
                        verbose=False
                        )
        if self.gdb_enabled:
            self.gdb_port = 1234
            self.test_command.append('-s')
            self.test_command.append('-S')
            print('Testing with Gdb enabled at port {}'.format(self.gdb_port))

    def validate_test_success(self, rc, stdout, stderr, execution_time) -> bool:
        assert rc != 0, "Error: rc is zero, something changed regarding the returncodes from qemu"
        if rc == 1:
            print("Test failed due to QEMU error. Is QEMU installed?", file=sys.stderr)
            return False
        elif rc != 33:
            # Since we are using asserts, tests should mostly fail due to a panic
            # However, other kinds of test errors using the debug_exit of qemu are also possible
            print("Test failed due to error returncode: {}".format(rc), file=sys.stderr)
            return False
        return super().validate_test_success(rc, stdout, stderr, execution_time)


class UhyveTestRunner(TestRunner):
    def __init__(self, 
                test_exe_path: str, 
                timeout_seconds: int,
                uhyve_path=None, 
                num_cores=1, 
                memory_in_megabyte=512, 
                gdb_enabled=False,
                verbose=False):
        if platform.system() == 'Windows':
            print("Error: using uhyve requires kvm. Please use Linux or Mac OS", file=sys.stderr)
            raise OSError
        if uhyve_path is None:
            uhyve_path = 'uhyve'
        else:
            assert os.path.isfile(uhyve_path), "Invalid uhyve path"
            self.uhyve_path = os.path.abspath(uhyve_path)
        test_command = [uhyve_path]
        if verbose:
            test_command.append('-v')
        test_command.append(test_exe_path)
        super().__init__(test_command=test_command, timeout_seconds=timeout_seconds, num_cores=num_cores, memory_in_megabyte=memory_in_megabyte,
                         gdb_enabled=gdb_enabled, verbose=verbose)
        # ToDo: This could be done a lot nicer if we could use flags to pass these options to uhyve
        if gdb_enabled or num_cores != 1:
            self.custom_env = os.environ.copy()
        if gdb_enabled:
            self.gdb_port = 1234  # ToDo: Add parameter to customize this

            self.custom_env['HERMIT_GDB_PORT'] = str(self.gdb_port)
            print('Testing with Gdb enabled at port {}'.format(self.gdb_port))
        if num_cores != 1:
            self.custom_env['HERMIT_CPUS'] = str(num_cores)

    def validate_test_success(self, rc, stdout, stderr, execution_time) -> bool:
        if rc != 0:
            print("Test failed due to error returncode: {}".format(rc), file=sys.stderr)
            return False
        else:
            return super().validate_test_success(rc, stdout, stderr, execution_time)


# ToDo: Think about how to pass information about how many tests an executable executed back to the runner
#  Maybe something like `[TEST_INFO]` at the start of a line?
def validate_stdout(stdout):
    """

    :param stdout:
    :return: true if stdout does not indicate test failure
    """
    # Todo: support should_panic tests (Implementation on hermit side with custom panic handler)
    if "!!!PANIC!!!" in stdout:
        return False
    return True


def clean_test_name(name: str):
    if name.endswith('.exe'):
        name = name.replace('.exe', '')
    # Remove the hash from the name
    parts = name.split('-')
    if len(parts) > 1:
        try:
            _hex = int(parts[-1], base=16)  # Test if last element is hex hash
            clean_name = "-".join(parts[:-1])  # Rejoin with '-' as separator in case test has it in filename
        except ValueError as e:
            print(e)
            clean_name = name  # In this case name doesn't contain a hash, so don't modify it any further
        return clean_name
    return name

    # Start "main"


assert sys.version_info[0] == 3, "Python 3 is required to run this script"
assert sys.version_info[1] >= 6, "Currently at least Python 3.6 is required for this script"

parser = argparse.ArgumentParser(description='See documentation of cargo test runner for custom test framework')
hypervisor_group = parser.add_mutually_exclusive_group()
hypervisor_group.add_argument('--bootloader_path', type=str, help="Provide path to hermit bootloader, implicitly "
                                                                  "switches to QEMU execution")
hypervisor_group.add_argument('--uhyve_path', type=str, default=None, help="Custom Path to uhyve if it is not in PATH")
parser.add_argument('runner_args', type=str, nargs='*')
parser.add_argument('-v', '--verbose', action='store_true', help="Always prints stdout/stderr of test")
parser.add_argument('-vv', '--veryverbose', action='store_true', help='verbose and additionally runs test verbosely')
parser.add_argument('--gdb', action='store_true', help='Enables gdb on port 1234 and stops at test executable '
                                                       'entrypoint')
parser.add_argument('--num_cores', type=int, default=1, help="Number of CPU cores the test should run on")
parser.add_argument('--timeout', type=int, default=300, help="Timeout in seconds for the test process.")

args = parser.parse_args()
print("Arguments: {}".format(args.runner_args))

# The last argument is the executable, all other arguments are ignored for now - ToDo: recheck this
test_exe = args.runner_args[-1]
assert isinstance(test_exe, str)
assert os.path.isfile(test_exe)  # If this fails likely something about runner args changed
assert args.timeout > 0, "Timeout must be a positive integer" # Todo: add range checking directly into parser.add_argument
# ToDo: Add additional test based arguments for qemu / uhyve

test_name = os.path.basename(test_exe)
test_name = clean_test_name(test_name)

if args.bootloader_path is not None:
    test_runner = QemuTestRunner(test_exe, 
                    timeout_seconds=args.timeout, 
                    bootloader_path = args.bootloader_path, 
                    gdb_enabled=args.gdb, 
                    num_cores=args.num_cores
                    )
elif platform.system() == 'Windows':
    print("Error: using uhyve requires kvm. Please use Linux or Mac OS, or use qemu", file=sys.stderr)
    exit(-1)
else:
    test_runner = UhyveTestRunner(test_exe, 
                    timeout_seconds=args.timeout, 
                    verbose=args.veryverbose, 
                    gdb_enabled=args.gdb, 
                    num_cores=args.num_cores,
                    uhyve_path=args.uhyve_path)

if test_name == "hermit":
    print("Executing the Unittests is currently broken... Skipping Test NOT marking as failed")
    # print("Note: If you want to execute all tests, consider adding the '--no-fail-fast' flag")
    print("If you wish to manually execute the Unittests, you can simply run:")
    print("`{}`".format(' '.join(test_runner.test_command)))
    exit(0)

rc, stdout, stderr, execution_time, timed_out = test_runner.run_test()
if timed_out:
    print('Test {} did not finish before timeout of {} seconds'.format(test_name, args.timeout))
    print("Test failed - Dumping Stderr:\n{}\n\nDumping Stdout:\n{}\n".format(stderr, stdout), file=sys.stderr)
    exit(1)
test_ok = test_runner.validate_test_success(rc, stdout, stderr, execution_time)
if test_ok :
    print("Test Ok: {} - runtime: {} seconds".format(test_name, execution_time))
    if args.verbose or args.veryverbose:
        print("Test {} stdout: {}".format(test_name, stdout))
        print("Test {} stderr: {}".format(test_name, stderr))
    exit(0)
else:
    print("Test failed: {} - runtime: {} seconds".format(test_name, execution_time / (10 ** 9)))
    print("Test failed - Dumping Stderr:\n{}\n\nDumping Stdout:\n{}\n".format(stderr, stdout), file=sys.stderr)
    exit(1)

# Todo: improve information about the test:
#       Maybe we could produce a JUnit XML by iteratively generating it for every call of this script
#       Sounds complex though
