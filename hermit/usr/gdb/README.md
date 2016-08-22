GDB-scripts for HermitCore awareness
====================================

The scripts located in this folder can be used when debugging HermitCore
applications to gain more insight into kernel internals.

To use the scripts you have to load them from inside GDB:

```
(gdb) source ../gdb/hermit-gdb.py
```

## Examples

Show state of all tasks:

```
(gdb) hermit-ps 
 ID | STATE | CPU | PRIO |      STACK | INSTRUCTION POINTER
--------------------------------------------------------------------
  0 |  IDL  |   0 |    0 |   0x87f000 | 0x81108c <rollback>
  1 |  IDL  |   1 |    0 |   0x881000 | 0x81108c <rollback>
  2 |  BLK  |   0 |    8 |    0xd6000 | 0x81108c <rollback>
  3 |  BLK  |   0 |   16 |    0xf8000 | 0x81108c <rollback>
  4 |  RUN  |   1 |    8 |   0x10e000 | 0x981ee8 <thread_func1+40>
  5 |  RUN  |   0 |    8 |   0x124000 | 0x981e98 <thread_func2+40>
```

Investigate state of specific task:

```
(gdb) print $hermit_task_by_id(2)
$1 = {id = 2, status = 3, last_core = 0, last_stack_pointer = 0x107e60, stack = 0xf8000, ist_addr = 0xe7000, flags = 0 '\000', 
  prio = 16 '\020', timeout = 883, start_tick = 207, heap = 0x0, parent = 0, next = 0x0, prev = 0x0, tls_addr = 12204928, 
  tls_size = 24, lwip_err = 0, signal_handler = 0x0, fpu = {fsave = {cwd = 0, swd = 0, twd = 0, fip = 0, fcs = 0, foo = 0, fos = 0, 
      st_space = {0 <repeats 20 times>}, status = 0}, fxsave = {cwd = 0, swd = 0, twd = 0, fop = 0, {{rip = 0, rdp = 0}, {fip = 0, 
          fcs = 0, foo = 0, fos = 0}}, mxcsr = 0, mxcsr_mask = 0, st_space = {0 <repeats 32 times>}, xmm_space = {
        0 <repeats 64 times>}, padding = {0 <repeats 12 times>}, {padding1 = {0 <repeats 12 times>}, sw_reserved = {
          0 <repeats 12 times>}}}, xsave = {fxsave = {cwd = 0, swd = 0, twd = 0, fop = 0, {{rip = 0, rdp = 0}, {fip = 0, fcs = 0, 
            foo = 0, fos = 0}}, mxcsr = 0, mxcsr_mask = 0, st_space = {0 <repeats 32 times>}, xmm_space = {0 <repeats 64 times>}, 
        padding = {0 <repeats 12 times>}, {padding1 = {0 <repeats 12 times>}, sw_reserved = {0 <repeats 12 times>}}}, hdr = {
        xstate_bv = 0, xcomp_bv = 0, reserved = {0, 0, 0, 0, 0, 0}}, ymmh = {ymmh_space = {0 <repeats 64 times>}}}}}
```

Show registered signal handlers (by `hermit_signal(signal_handler_t handler)`).
Note that these are not signal handlers registered by newlib 
(`signal(int sig, _sig_func_ptr func)`).

```
(gdb) hermit-ls-sighandler
 ID | Signal Handler
------------------------------
  0 | 0x0
  1 | 0x0
  2 | 0x989660 <signal_dispatcher>
  3 | 0x0
  4 | 0x989660 <signal_dispatcher>
  5 | 0x989660 <signal_dispatcher>
```
