#
# gdb helper commands and functions for HermitCore debugging
#
#  task & thread tools
#
# Copyright (c) Siemens AG, 2011-2013
# Copyright (c) RWTH-Aaachen, 2016
#
# Authors:
#  Jan Kiszka <jan.kiszka@siemens.com>
#  Daniel Krebs <github@daniel-krebs.net>
#
# Inspired by prior work of Jan Kiszka and adapted by Daniel Krebs
# for HermitCore.
#
# This work is licensed under the terms of the GNU GPL version 2.
#

import gdb


def task_lists():
    task_table = gdb.parse_and_eval("task_table")

    for i in range(task_table.type.range()[1]):
        task = task_table[i]
        if task['status'] != 0:
            yield task

def get_task_by_pid(pid):
    for task in task_lists():
        if int(task['id']) == pid:
            return task
    return None


class HermitTaskByIdFunc(gdb.Function):
    """Find HermitCore task by ID and return the task_t variable.

$hermit_task_by_pid(ID): Given ID, iterate over all tasks of the target and
return that task_t variable which PI matches."""

    def __init__(self):
        super(HermitTaskByIdFunc, self).__init__("hermit_task_by_id")

    def invoke(self, pid):
        task = get_task_by_pid(pid)
        if task:
            return task
        else:
            raise gdb.GdbError("No task with ID " + str(pid))


HermitTaskByIdFunc()

def addressToSymbol(addr):
    s = gdb.execute("info symbol 0x%x" % addr, to_string=True)
    if 'No symbol matches' in s:
        return ''
    else:
        return s.split(' in')[0].replace(' ', '')

class HermitPs(gdb.Command):
    """Dump Hermit tasks."""

    def __init__(self):
        super(HermitPs, self).__init__("hermit-ps", gdb.COMMAND_DATA)

    def invoke(self, arg, from_tty):
        # see include/hermit/task_types.h
        status_desc = {1: 'RDY', 2: 'RUN', 3: 'BLK', 4: 'FIN', 5: 'IDL'}

        rowfmt = "{id:>3} | {status:^5} | {last_core:>3} | {prio:>4} | {stack:>10} | {rip:<28}\n"

        header = rowfmt.format(id='ID', status='STATE', last_core='CPU',
                               prio='PRIO', stack='STACK',
                               rip='INSTRUCTION POINTER')

        gdb.write(header)
        gdb.write((len(header) - 1) * '-' + '\n')

        inferior = gdb.selected_inferior()
        currentInferiorThread = gdb.selected_thread()

        for task in task_lists():

            task_status = status_desc[int(task["status"])]

            if task_status == 'RUN':
                # switch to inferior thread (cpu) that this task is running on
                for inferiorThread in inferior.threads():
                    # GDB starts indexing at 1
                    coreId = inferiorThread.num - 1
                    if coreId == task['last_core']:
                        inferiorThread.switch()
                        break

                # get instruction pointer and switch back
                rip = str(gdb.parse_and_eval('$pc'))
                currentInferiorThread.switch()

            else:
                # find instruction pointer in saved stack
                rip_addr = task['last_stack_pointer'] + 25
                rip_val = int(rip_addr.dereference())
                # try to resolve a symbol
                rip_sym = addressToSymbol(rip_val)
                rip = "0x%x" % rip_val
                if rip_sym:
                    rip += " <%s>" % rip_sym

            gdb.write(rowfmt.format(
                id=int(task["id"]),
                status=task_status,
                rip=str(rip),
                prio=int(task['prio']),
                last_core=int(task['last_core']),
                stack="{:#x}".format(int(task['stack']))
                ))

HermitPs()


class HermitLsSighandler(gdb.Command):
    """List signal handlers per tasks."""

    def __init__(self):
        super(HermitLsSighandler, self).__init__("hermit-ls-sighandler", gdb.COMMAND_DATA)

    def invoke(self, arg, from_tty):

        rowfmt = "{id:>3} | {signal_handler:<24}\n"

        header = rowfmt.format(id='ID', signal_handler='Signal Handler')

        gdb.write(header)
        gdb.write((len(header) - 1) * '-' + '\n')

        for task in task_lists():

            gdb.write(rowfmt.format(
                id=int(task["id"]),
                signal_handler=str(task['signal_handler']),
                ))

HermitLsSighandler()



def stripSymbol(value):
    s = "%s" % value
    return s.split(' ')[0]

class HermitTaskState:
    def __init__(self, address = None):
        import re
        self.info_reg_regex = re.compile("(?P<register>[\w]+)\s+(?P<value>0x[0-9a-f]+).*")

        if address:
            self.address = address

            self.registers = {
                'gs': self.address + 0,
                'fs': self.address + 1,
                'r15': self.address + 2,
                'r14': self.address + 3,
                'r13': self.address + 4,
                'r12': self.address + 5,
                'r11': self.address + 6,
                'r10': self.address + 7,
                'r9':  self.address + 8,
                'r8':  self.address + 9,
                'rdi': self.address + 10,
                'rsi': self.address + 11,
                'rbp': self.address + 12,
                'rsp': self.address + 13,
                'rbx': self.address + 14,
                'rdx': self.address + 15,
                'rcx': self.address + 16,
                'rax': self.address + 17,
                # int_no
                # error
                'rip':    self.address + 20,
                'cs':     self.address + 21,
                'eflags': self.address + 22,
                # userrsp
                'ss':     self.address + 24,
            }

            # make nice strings out of register values
            for register, valptr in self.registers.items():
                self.registers[register] = stripSymbol(valptr.dereference())

        else:
            self.address = False
            self.info_registers = gdb.execute('info registers', to_string=True)
            self.registers = {}
            for line in self.info_registers.split('\n'):
                match = self.info_reg_regex.match(line)
                if match:
                    self.registers[match.group('register')] = match.group('value')

    def switch(self):
        for register, value in self.registers.items():
            try:
                gdb.execute("set $%s = %s" % (register, value))
            except:
                print("Cannot restore %s=%s, skipping ..." % (register, value))


class HermitTaskBacktrace(gdb.Command):
    """Show backtrace for HermitCore task.

Usage: hermit-bt ID"""

    def __init__(self):
        super(HermitTaskBacktrace, self).__init__("hermit-bt", gdb.COMMAND_DATA)

    def invoke(self, arg, from_tty):
        argv = gdb.string_to_argv(arg)
        if len(argv) != 1:
            raise gdb.GdbError("hermit-bt takes one argument")

        task = get_task_by_pid(int(argv[0]))

        if task['status'] == 2:
            gdb.execute('bt')
            return

        current_state = HermitTaskState()

        task_state = HermitTaskState(task['last_stack_pointer'])

        try:
            task_state.switch()
            gdb.execute('bt')
        finally:
            current_state.switch()

HermitTaskBacktrace()

original_state = {}

def saveCurrentState(state):
    curr_thread = gdb.selected_thread()
    for thread in gdb.selected_inferior().threads():
        if not thread.num in state:
            thread.switch()
            state[thread.num] = HermitTaskState()
    curr_thread.switch()

def restoreCurrentState(state):
    curr_thread = gdb.selected_thread()
    for thread in gdb.selected_inferior().threads():
        if thread.num in state:
            thread.switch()
            state[thread.num].switch()
    curr_thread.switch()
    state = {}

class HermitSwitchContext(gdb.Command):
    """Switch current context to given HermitCore task

Usage: hermit-switch-context ID"""

    def __init__(self):
        super(HermitSwitchContext, self).__init__("hermit-switch-context", gdb.COMMAND_DATA)

    def invoke(self, arg, from_tty):
        global original_state

        argv = gdb.string_to_argv(arg)
        if len(argv) != 1:
            raise gdb.GdbError("hermit-switch-context takes one argument")

        # save original state to go back to it later
        saveCurrentState(original_state)

        task = get_task_by_pid(int(argv[0]))

        # switch current inferior thread to where task has run last
        for thread in gdb.selected_inferior().threads():
            if (thread.num - 1) == task['last_core']:
                thread.switch()
                break

        # apply it's state
        task_state = HermitTaskState(task['last_stack_pointer'])
        task_state.switch()

HermitSwitchContext()


class HermitRestoreContext(gdb.Command):
    """Restore context to state before it was switched

Usage: hermit-restore-context"""

    def __init__(self):
        super(HermitRestoreContext, self).__init__("hermit-restore-context", gdb.COMMAND_DATA)

    def invoke(self, arg, from_tty):
        global original_state

        restoreCurrentState(original_state)

HermitRestoreContext()
