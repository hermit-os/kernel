#!/usr/bin/env python3

import time
import argparse
import subprocess
from subprocess import Popen, PIPE, STDOUT
import os, os.path

SMP_CORES = 1  # Number of cores
MEMORY_MB = 64  # amount of memory
# Path if libhermit-rs was checked out via rusty-hermit repository
BOOTLOADER_PATH = '../loader/target/x86_64-unknown-hermit-loader/debug/rusty-loader'


# ToDo add test dependent section for custom kernel arguments / application arguments
# Idea: Use TOML format to specify things like should_panic, expected output
# Parse test executable name and check tests directory for corresponding toml file
# If it doesn't exist just assure that the return code is not a failure

# ToDo Think about always being verbose, or hiding the output
def run_test(process_args):
    print(os.getcwd())
    abs_bootloader_path = os.path.abspath(BOOTLOADER_PATH)
    print("Abspath: ", abs_bootloader_path)
    p = Popen(process_args, stdout=PIPE, stderr=STDOUT, text=True)
    output: str = ""
    for line in p.stdout:
        dec_line = line
        output += dec_line
        #print(line, end='')  # stdout will already contain line break
    rc = p.wait()
    # ToDo: add some timeout
    return rc, output


def validate_test(returncode, output, test_exe_path):
    print("returncode ", returncode)
    # ToDo handle expected failures
    if returncode != 0:
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
                       '-cpu', 'qemu64,apic,fsgsbase,rdtscp,xsave,fxsr'
                       ]
ok_tests: int = 0
failed_tests: int = 0
# This is assuming test_runner only passes executable files as parameters
for arg in args.runner_args:
    assert isinstance(arg, str)
    curr_qemu_arguments = qemu_base_arguments.copy()
    # ToDo: assert that arg is a path to an executable before calling qemu
    # ToDo: Add addional test based arguments for qemu / uhyve
    curr_qemu_arguments.extend(['-initrd', arg])
    rc, output = run_test(curr_qemu_arguments)
    test_ok = validate_test(rc, output, arg)
    test_name = os.path.basename(arg)
    test_name = clean_test_name(test_name)
    if test_ok:
        print("Test Ok: {}".format(test_name))
        ok_tests += 1
    else:
        print("Test failed: {}".format(test_name))
        failed_tests += 1
    #Todo: improve information about the test

print("{} from {} tests successful".format(ok_tests, ok_tests + failed_tests))
# todo print something ala x/y tests failed etc.
#  maybe look at existing standards (TAP?)
#  - TAP: could use tappy to convert to python style unit test output (benefit??)

if failed_tests == 0:
    exit(0)
else:
    exit(1)





