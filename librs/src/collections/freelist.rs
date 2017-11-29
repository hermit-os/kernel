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

use alloc::rc::Rc;
use core::cell::RefCell;
use collections::doublylinkedlist::{DoublyLinkedList, Node};
use synch::spinlock::SpinlockIrqSave;


enum DeallocationListWalkResult {
	Done,
	InsertBefore(Rc<RefCell<Node<FreeListEntry>>>),
	InsertTail,
}

pub struct FreeListEntry {
	pub start: usize,
	pub end: usize,
}

pub struct FreeList {
	pub list: SpinlockIrqSave<DoublyLinkedList<FreeListEntry>>,
}

impl FreeList {
	pub const fn new() -> Self {
		Self { list: SpinlockIrqSave::new(DoublyLinkedList::new()) }
	}

	pub fn allocate(&self, size: usize) -> Result<usize, ()> {
		debug!("Allocating {} bytes from Free List {:#X}", size, self as *const Self as usize);

		let mut result = Err(());
		let mut node_to_remove: Option<Rc<RefCell<Node<FreeListEntry>>>> = None;
		let mut locked_list = self.list.lock();

		// Find a region in the Free List that has at least the requested size.
		for m in locked_list.iter() {
			let (region_start, region_size) = {
				let borrowed = m.borrow();
				(borrowed.value.start, borrowed.value.end - borrowed.value.start)
			};

			if region_size > size {
				// We have found a region that is larger than the requested size.
				// Return the address to the beginning of that region and shrink the region by that size.
				m.borrow_mut().value.start += size;
				result = Ok(region_start);
				break;
			} else if region_size == size {
				// We have found a region that has exactly the requested size.
				// Return the address to the beginning of that region and mark it for deletion from the list.
				node_to_remove = Some(m);
				result = Ok(region_start);
				break;
			}
		}

		// Remove any node marked for deletion from the list.
		// Preserve the memory allocated for the node, so we can control when it is deallocated.
		let old_node = match node_to_remove {
			Some(node) => locked_list.remove(node),
			None => (None, None)
		};

		// First unlock the list, then drop the memory behind the deleted node.
		// Dropping may call deallocation operations, which need an unlocked list.
		drop(locked_list);
		drop(old_node);

		result
	}

	fn deallocation_list_walk(locked_list: &mut DoublyLinkedList<FreeListEntry>, address: usize, end: usize) -> DeallocationListWalkResult {
		// The Free List is sorted from low to high addresses.
		// So find the position where our deallocated address belongs.
		for m in locked_list.iter() {
			let (region_start, region_end) = {
				let borrowed = m.borrow();
				(borrowed.value.start, borrowed.value.end)
			};

			if region_start == end {
				// The deallocated memory extends this free memory region to the left.
				m.borrow_mut().value.start = address;
				return DeallocationListWalkResult::Done;
			} else if region_end == address {
				// The deallocated memory extends this free memory region to the right.
				m.borrow_mut().value.end = end;
				return DeallocationListWalkResult::Done;
			} else if end < region_start {
				// The deallocated memory does not extend any memory region and needs an own entry in the Free List.
				// We search the list from low to high addresses and insert us before the first entry that has a
				// higher address than us.
				return DeallocationListWalkResult::InsertBefore(m);
			}
		}

		// We could not find an entry with a higher address than us.
		// So we become the new last entry in the list.
		DeallocationListWalkResult::InsertTail
	}

	pub fn deallocate(&self, address: usize, size: usize) {
		debug!("Deallocating {} bytes at {:#X} from Free List {:#X}", size, address, self as *const Self as usize);

		let end = address + size;
		let mut locked_list = self.list.lock();

		match Self::deallocation_list_walk(&mut locked_list, address, end) {
			DeallocationListWalkResult::InsertBefore(_node_before) => {
				// We need to insert a new entry into the Free List.
				// So unlock the list first and then allocate memory for a new entry.
				drop(locked_list);
				let new_node = Node::new(FreeListEntry { start: address, end: end });
				let mut locked_list = self.list.lock();

				// As we have allocated memory for a new entry, our returned node for insert_before may no longer be valid.
				// This is why we unfortunately have to do the list walk again on the updated Free List.
				match Self::deallocation_list_walk(&mut locked_list, address, end) {
					DeallocationListWalkResult::InsertBefore(node_before) => {
						// Finally, we can insert the entry at the right position.
						locked_list.insert_before(new_node, node_before)
					},

					DeallocationListWalkResult::InsertTail => {
						// Get the latest list tail and insert us after it, making us the new list tail.
						let tail = locked_list.tail().unwrap();
						locked_list.insert_after(new_node, tail);
					},

					_ => {}
				}
			},

			DeallocationListWalkResult::InsertTail => {
				// We need to insert a new entry into the Free List.
				// So unlock the list first and then allocate memory for a new entry.
				drop(locked_list);
				let new_node = Node::new(FreeListEntry { start: address, end: end });
				let mut locked_list = self.list.lock();

				// Get the latest list tail and insert us after it, making us the new list tail.
				let tail = locked_list.tail().unwrap();
				locked_list.insert_after(new_node, tail);
			},

			_ => {}
		}
	}
}
