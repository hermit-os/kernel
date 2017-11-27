// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use collections::DoublyLinkedList;


pub struct FreeListEntry {
	pub start: usize,
	pub end: usize,
}

pub struct FreeList {
	pub list: DoublyLinkedList<FreeListEntry>,
}

impl FreeList {
	pub const fn new() -> Self {
		Self { list: DoublyLinkedList::new() }
	}

	pub fn allocate(&mut self, size: usize) -> Result<usize, ()> {
		debug!("Allocating {} bytes from Free List {:#X}", size, self as *const Self as usize);

		for m in self.list.iter() {
			let (region_start, region_size) = {
				let borrowed = m.borrow();
				(borrowed.value.start, borrowed.value.end - borrowed.value.start)
			};

			if region_size > size {
				m.borrow_mut().value.start += size;
				debug!("resizing existing and returning {:#X}", region_start);
				return Ok(region_start);
			} else if region_size == size {
				self.list.remove(m);
				debug!("removing existing and returning {:#X}", region_start);
				return Ok(region_start);
			}
		}

		Err(())
	}

	pub fn deallocate(&mut self, address: usize, size: usize) {
		debug!("Deallocating {} bytes at {:#X} from Free List {:#X}", size, address, self as *const Self as usize);
		let end = address + size;

		for m in self.list.iter() {
			let (region_start, region_end) = {
				let borrowed = m.borrow();
				(borrowed.value.start, borrowed.value.end)
			};

			debug!("DEALLOCATE - region_start: {:#X}, region_end: {:#X}, address: {:#X}, size: {:#X}", region_start, region_end, address, size);

			if region_start == end {
				// The deallocated memory extends this free memory region to the left.
				m.borrow_mut().value.start = address;
				return;
			} else if region_end == address {
				// The deallocated memory extends this free memory region to the right.
				m.borrow_mut().value.end = end;
				return;
			} else if end < region_start {
				// The deallocated memory does not extend any memory region and needs an own entry in the Free List.
				// We search the list from low to high addresses and insert us before the first entry that has a
				// higher address than us.
				let entry = FreeListEntry { start: address, end: end };
				self.list.insert_before(entry, m);
				return;
			}
		}

		// We could not find an entry with a higher address than us.
		// So we become the new last entry in the list.
		let entry = FreeListEntry { start: address, end: end };
		let tail = self.list.tail().unwrap();
		self.list.insert_after(entry, tail);
	}
}
