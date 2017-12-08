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
use core::mem;
use mm;


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

		// Find a region in the Free List that has at least the requested size.
		for node in self.list.iter() {
			let (region_start, region_size) = {
				let borrowed = node.borrow();
				(borrowed.value.start, borrowed.value.end - borrowed.value.start)
			};

			if region_size > size {
				// We have found a region that is larger than the requested size.
				// Return the address to the beginning of that region and shrink the region by that size.
				node.borrow_mut().value.start += size;
				return Ok(region_start);
			} else if region_size == size {
				// We have found a region that has exactly the requested size.
				// Return the address to the beginning of that region and move the node into the pool for deletion or reuse.
				self.list.remove(node.clone());
				unsafe { mm::POOL.list.push(node); }
				return Ok(region_start);
			}
		}

		Err(())
	}

	pub fn deallocate(&mut self, address: usize, size: usize) {
		debug!("Deallocating {} bytes at {:#X} from Free List {:#X}", size, address, self as *const Self as usize);

		let end = address + size;
		let mut iter = self.list.iter();

		while let Some(node) = iter.next() {
			let (region_start, region_end) = {
				let borrowed = node.borrow();
				(borrowed.value.start, borrowed.value.end)
			};

			if region_start == end {
				// The deallocated memory extends this free memory region to the left.
				node.borrow_mut().value.start = address;
				return;
			} else if region_end == address {
				// The deallocated memory extends this free memory region to the right.
				// Check if it can even reunite with the next region.
				if let Some(next_node) = iter.next() {
					let (next_region_start, next_region_end) = {
						let borrowed = node.borrow();
						(borrowed.value.start, borrowed.value.end)
					};

					if next_region_start == end {
						// It can reunite, so let the current region span over the reunited region and move the duplicate node
						// into the pool for deletion or reuse.
						node.borrow_mut().value.end = next_region_end;
						self.list.remove(next_node.clone());
						unsafe { mm::POOL.list.push(next_node); }
						return;
					}
				}

				// It cannot reunite, so just extend this region to the right and we are done.
				node.borrow_mut().value.end = end;
				return;
			} else if end < region_start {
				// The deallocated memory does not extend any memory region and needs an own entry in the Free List.
				// Get that entry from the node pool.
				// We search the list from low to high addresses and insert us before the first entry that has a
				// higher address than us.
				let new_node = unsafe { mm::POOL.list.head().unwrap() };
				unsafe { mm::POOL.list.remove(new_node.clone()); }

				{
					let mut new_node_borrowed = new_node.borrow_mut();
					new_node_borrowed.value.start = address;
					new_node_borrowed.value.end = end;
				}

				self.list.insert_before(new_node, node);
				return;
			}
		}

		// We could not find an entry with a higher address than us.
		// So we become the new last entry in the list. Get that entry from the node pool.
		let new_node = unsafe { mm::POOL.list.head().unwrap() };
		unsafe { mm::POOL.list.remove(new_node.clone()); }

		{
			let mut new_node_borrowed = new_node.borrow_mut();
			new_node_borrowed.value.start = address;
			new_node_borrowed.value.end = end;
		}

		if let Some(tail) = self.list.tail() {
			self.list.insert_after(new_node, tail);
		} else {
			self.list.push(new_node);
		}
	}

	pub fn print_information(&self, header: &str) {
		infoheader!(header);

		for node in self.list.iter() {
			let (region_start, region_end) = {
				let borrowed = node.borrow();
				(borrowed.value.start, borrowed.value.end)
			};
			info!("{:#016X} - {:#016X}", region_start, region_end);
		}

		infofooter!();
	}
}
