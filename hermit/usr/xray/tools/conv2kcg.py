#!/usr/bin/env python3

"""Copyright (c) 2016, Daniel Krebs, RWTH Aachen University

All rights reserved.
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:
   * Redistributions of source code must retain the above copyright
     notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above copyright
     notice, this list of conditions and the following disclaimer in the
     documentation and/or other materials provided with the distribution.
   * Neither the name of the University nor the names of its contributors
     may be used to endorse or promote products derived from this
     software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE."""

import re
import logging
import pprint
import sys
import os

if sys.hexversion < 0x03040000:
	raise Exception("At least Python 3.4.x is required")
else:
	from enum import Enum, unique


class CallTree:
	def __init__(self, funcName, funcAddr, totalTicks):
		self.name = funcName
		self.addr = funcAddr
		self.ticks = totalTicks

		self.calls = []

	def call(self, funcName, funcAddr, totalTicks):
		callee = CallTree(funcName, funcAddr, totalTicks)
		self.calls.append(callee)
		return callee

	def toString(self, caller, depth):
		indent = ' ' * depth
		out = "%s%s [%d]\n" % (indent, caller.name, caller.ticks)
		for callee in caller.calls:
			out += "  %s%s" % (indent, self.toString(callee, depth + 1))
		return out

	def __repr__(self):
		return self.toString(self, 0)


class Frame:
	def __init__(self, name, totalTicks, captureSize):
		self.name = name
		self.totalTicks = totalTicks
		self.captureSize = captureSize

		self.callTree = CallTree('_root_', '0x0', self.totalTicks)

	def call(self, *args, **kwargs):
		return self.callTree.call(*args, **kwargs)

	def __repr__(self):
		return str(self.callTree)


class ParsingState:
	def __init__(self):
		self.last_call_depth = 0
		self.callers = {}
		self.call_count = {}

		# global dict that stores all frames
		self.frames = {}


# 0x008D3240	  156321813		100.0	   benchmark
frameCallLineRegex = re.compile('(?P<address>0x[0-9A-F]+)\s+(?P<cycles>\d+)\s+(?P<percentage>\d+\.\d+)      (?P<depth>\s*)(?P<name>[\w\(\)]+)\s*(?P<annotation>.*)')

# label PARALLEL
frameStartRegex = re.compile('label (?P<label>\w+)')

# Frame#		Total Ticks		 Capture size	 Annotations   Label
headerStartRegex = re.compile('^Frame#.*')

#	   0		  156322740			   916352			  25   PARALLEL
headerFrameLineRegex = re.compile('\s+(?P<id>\d+)\s+(?P<ticks>\d+)\s+(?P<size>\d+)\s+(?P<annotations>\d+)\s+(?P<label>\w+)')


def headerStarted(line):
	match = headerStartRegex.match(line)
	if match:
		return True
	else:
		return False


def parseHeader(line):
	match = headerFrameLineRegex.match(line)
	if match:
		return Frame(match.group('label'), int(match.group('ticks')), int(match.group('size')))
	return None


def frameStarted(line):
	match = frameStartRegex.match(line)
	if match:
		return match.group('label')
	else:
		return None


def parseFrame(state, frameName, line):
	match = frameCallLineRegex.match(line)
	if match:
		address = match.group('address')
		cycles = match.group('cycles')
		percentage = match.group('percentage')
		depth = match.group('depth')
		funcName = match.group('name')
		annotation = match.group('annotation')
		logging.debug("%s @ %s: %s cycles" % (funcName, address, cycles))

		# this is deprecated
		if not address in state.call_count:
			state.call_count[address] = {'name': funcName, 'count': 0}
		state.call_count[address]['count'] += 1

		frame = state.frames[frameName]

		state.last_call_depth = len(depth)

		caller = None
		if state.last_call_depth == 0:
			caller = frame
		else:
			caller = state.callers[state.last_call_depth - 1]

		state.callers[state.last_call_depth] = caller.call(funcName, address, int(cycles))

	else:
		logging.debug("Line did not match: '%s'" % line)



def parseReport(report_file, parsingState):

	@unique
	class States(Enum):
		FindFrame = 1
		ParseFrame = 2
		FindHeader = 3
		ParseHeader = 4

	state = States.FindHeader
	current_frame = None

	with open(report_file, "r") as file:
		for line in file:
			if state == States.FindFrame:
				logging.debug("Find frame")

				current_frame = frameStarted(line)
				if current_frame:
					logging.info("Found frame '%s' data" % current_frame)
					state = States.ParseFrame
					continue

			elif state == States.ParseFrame:
				logging.debug("Parse frame")

				if '===' in line[0:3]:
					logging.info("Frame '%s' complete" % current_frame)
					# pp = pprint.PrettyPrinter(indent = 2)
					# pp.pprint(call_count)
					# pp.pprint(frames[current_frame])
					state = States.FindFrame
					continue
				else:
					parseFrame(parsingState, current_frame, line)

			elif state == States.FindHeader:
				logging.debug("Find Header")

				if headerStarted(line):
					state = States.ParseHeader
					continue

			elif state == States.ParseHeader:
				logging.debug("Parse Header")

				frame = parseHeader(line)
				if frame:
					parsingState.frames[frame.name] = frame
				else:
					logging.debug("Ignore line '%s'" % line)

				if 'XRay:' in line[0:5]:
					logging.info("Parsing Header is done. Found %d frames" % len(parsingState.frames))
					state = States.FindFrame
					continue

			else:
				logging.error("Unknown state %s" % state)
				sys.exit(1)

	logging.info("Report file '%s' parsed completely." % report_file)


	def writeCallgrindHeader(f, totalTicks):
		f.write("positions: line\n")
		f.write("events: ticks\n")
		f.write("summary: %d\n" % totalTicks)
		f.write("\n")

	def dumpCallTree(f, callTree):
		f.write("fl=%s_%s.c\n" % (callTree.name, callTree.addr))
		f.write("fn=%s_%s\n" % (callTree.name, callTree.addr))

		# calculate self cost
		selfCost = callTree.ticks
		for callee in callTree.calls:
			selfCost -= callee.ticks
		# self cost will be always first line
		f.write("1 %d\n" % selfCost)

		line = 2
		for callee in callTree.calls:
			# each function will be in it's own file for now
			# we need to include the address to disambiguate (null) functions
			f.write("cfl=%s_%s.c\n" % (callee.name, callee.addr))
			f.write("cfn=%s_%s\n" % (callee.name, callee.addr))
			# calls one time and functions are always on line zero
			f.write("calls=1 0\n")
			# cost: each function will be called on a new line
			f.write("%d %d\n" % (line, callee.ticks))

			line += 1

		# function is done
		f.write("\n")

		# now dump each callee
		for callee in callTree.calls:
			dumpCallTree(f, callee)

	def createCallgrindReport(filename, frame):

		with open(filename, "w") as f:
			writeCallgrindHeader(f, frame.totalTicks)
			dumpCallTree(f, frame.callTree)

	basepath = os.path.dirname(report_file)
	basename = os.path.basename(report_file)
	reportname = os.path.splitext(basename)[0]

	for name, frame in parsingState.frames.items():
		filename = "%s_%s.callgrind" % (reportname, name)
		filepath = os.path.join(basepath, filename)

		logging.info("Create callgrind file for frame '%s'" % name)
		logging.info("Writing to: %s" % filepath)

		createCallgrindReport(filepath, frame)


if __name__ == '__main__':
	import argparse

	parser = argparse.ArgumentParser(
		description=
				"""Convert XRay report to Callgrind to visualize with kCacheGrind.
				A new file will be created for each XRay frame next to the
				original report.""",
		epilog="Example: {} report.xray".format(__file__))

	parser.add_argument("xray_report", help="Report generated by XRay")

	parser.add_argument("-v", "--verbose",
						help="Be more verbose while parsing",
						action="store_true", default=False)
	parser.add_argument("-q", "--quiet",
						help="Only show errors",
						action="store_true", default=False)

	args = parser.parse_args()

	if not args.xray_report:
		logging.error("You must supply an XRay report file")
		sys.exit(1)
	elif not os.path.isfile(args.xray_report):
		logging.error("'%s' is not a file" % inputf)
		sys.exit(1)

	if args.verbose and args.quiet:
		logging.error("Argument 'verbose' contradicts 'quiet'.")
		sys.exit(1)

	loglevel = logging.INFO
	if args.verbose:
		loglevel = logging.DEBUG
	elif args.quiet:
		loglevel = logging.ERROR

	# setup logging to console
	logging.basicConfig(format='%(levelname)s:%(message)s', level=loglevel)

	state = ParsingState()
	parseReport(args.xray_report, state)

