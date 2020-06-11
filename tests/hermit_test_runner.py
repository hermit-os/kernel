#!/usr/bin/env python3

import argparse
import os
import os.path
import sys
import time
from subprocess import Popen, PIPE, STDOUT
import subprocess
import platform

SMP_CORES = 1  # Number of cores
MEMORY_MB = 256  # amount of memory
# Path if libhermit-rs was checked out via rusty-hermit repository
BOOTLOADER_PATH = '../loader/target/x86_64-unknown-hermit-loader/debug/rusty-loader'
USE_UHYVE = True   # ToDo: consider using a class and dynamic methods instead of global options
GDB = False


# ToDo add test dependent section for custom kernel arguments / application arguments
# Idea: Use TOML format to specify things like should_panic, expected output
# Parse test executable name and check tests directory for corresponding toml file
# If it doesn't exist just assure that the return code is not a failure

# ToDo Think about always being verbose, or hiding the output
def run_test_qemu(process_args):
    print(os.getcwd())
    abs_bootloader_path = os.path.abspath(BOOTLOADER_PATH)
    print("Abspath: ", abs_bootloader_path)
    start_time = time.time_ns()  # Note: Requires python >= 3.7
    p = Popen(process_args, stdout=PIPE, stderr=STDOUT, text=True)
    output: str = ""
    for line in p.stdout:
        dec_line = line
        output += dec_line
        print(line, end='')  # stdout will already contain line break
    rc = p.wait()
    end_time = time.time_ns()
    # ToDo: add some timeout
    return rc, output, end_time - start_time


def run_test_uhyve(kernel_path):
    assert os.path.isfile(kernel_path)
    process_args = ['uhyve', '-v', kernel_path]
    start_time = time.time_ns()  # Note: Requires python >= 3.7
    my_env = os.environ.copy()
    if GDB:
        my_env['HERMIT_GDB_PORT'] = '1234'
    p = subprocess.run(process_args, stdout=PIPE, stderr=STDOUT, text=True, env=my_env)
    end_time = time.time_ns()
    print(p.stdout)
    return p.returncode, p.stdout, end_time - start_time


def validate_test(returncode, output, test_exe_path):
    print("returncode ", returncode)
    # ToDo handle expected failures
    if not USE_UHYVE and returncode != 33:
        return False
    if USE_UHYVE and returncode != 0:
        return False
    # ToDo parse output for panic
    return True


def clean_test_name(name: str):
    if name.endswith('.exe'):
        name = name.replace('.exe', '')
    # Remove the hash from the name
    parts = name.split('-')
    if len(parts) > 1:
        try:
            _hex = int(parts[-1], base=16)  # Test if last element is hex hash
            clean_name = "-".join(parts[:-1])  # Rejoin with '-' as seperator in case test has it in filename
        except ValueError as e:
            print(e)
            clean_name = name  # In this case name doesn't contain a hash, so don't modify it any further
    return clean_name


assert sys.version_info[0] == 3, "Python 3 is required to run this script"
assert sys.version_info[1] >= 7, "Currently at least Python 3.7 is required for this script, If necessary this could " \
                                 "be reduced "
print("Test runner called")
parser = argparse.ArgumentParser(description='See documentation of cargo test runner for custom test framework')
parser.add_argument('runner_args', type=str, nargs='*')
args = parser.parse_args()
print("Arguments: {}".format(args.runner_args))

qemu_base_arguments = ['qemu-system-x86_64',
                       '-display', 'none',
                       '-smp', str(SMP_CORES),
                       '-m', str(MEMORY_MB) + 'M',
                       '-serial', 'stdio',
                       '-kernel', BOOTLOADER_PATH,
                       # skip initrd - it depends on test executable
                       '-cpu', 'qemu64,apic,fsgsbase,rdtscp,xsave,fxsr',
                       '-device', 'isa-debug-exit,iobase=0xf4,iosize=0x04',
                       ]
if GDB:
    qemu_base_arguments.append('-s')
    qemu_base_arguments.append('-S')
# The last argument is the executable, all other arguments are ignored for now
arg = args.runner_args[-1]
assert isinstance(arg, str)
curr_qemu_arguments = qemu_base_arguments.copy()
assert os.path.isfile(arg)      # If this fails likely something about runner args changed
# ToDo: Add addional test based arguments for qemu / uhyve
curr_qemu_arguments.extend(['-initrd', arg])
if USE_UHYVE:
    if platform.system() == 'Windows':
        print("Error: using uhyve requires kvm. Please use Linux or Mac OS")
        exit(-1)
    rc, output, rtime = run_test_uhyve(arg)
else:
    rc, output, rtime = run_test_qemu(curr_qemu_arguments)
test_ok = validate_test(rc, output, arg)
test_name = os.path.basename(arg)
test_name = clean_test_name(test_name)
if test_ok:
    print("Test Ok: {} - runtime: {} seconds".format(test_name, rtime / (10 ** 9)))
    exit(0)
else:
    print("Test failed: {} - runtime: {} seconds".format(test_name, rtime / (10 ** 9)))
    exit(1)
#Todo: improve information about the test

# todo print something ala x/y tests failed etc.
#  maybe look at existing standards (TAP?)
#  - TAP: could use tappy to convert to python style unit test output (benefit??)
