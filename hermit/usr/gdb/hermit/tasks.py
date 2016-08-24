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
                rip_addr = task['last_stack_pointer'] + 20
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

        inferior = gdb.selected_inferior()
        currentInferiorThread = gdb.selected_thread()

        for task in task_lists():

            gdb.write(rowfmt.format(
                id=int(task["id"]),
                signal_handler=str(task['signal_handler']),
                ))

HermitLsSighandler()
