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

use collections::{DoublyLinkedList, Node};
use mm::freelist::FreeListEntry;


/// A deallocation operation in a Free List may need a node from the pool.
/// As we use two free lists (for physical and virtual memories), we always need to guarantee a minimum of 2 nodes in the pool for any deallocation operation.
const MINIMUM_POOL_ENTRIES: usize = 2;


pub struct NodePool {
	pub list: DoublyLinkedList<FreeListEntry>,
	maintenance_in_progress: bool,
}

impl NodePool {
	pub const fn new() -> Self {
		Self {
			list: DoublyLinkedList::new(),
			maintenance_in_progress: false,
		}
	}

	pub fn maintain(&mut self) {
		// Prevent calling this function recursively (see below).
		if self.maintenance_in_progress {
			return;
		}

		debug!("Pool Maintenance!");
		self.maintenance_in_progress = true;

		// Keep the desired minimum number of entries in the pool and move the rest into the local nodes_to_remove list.
		// Note that our node pool changes during node removal, so we definitely want to work on a local list.
		let mut i = 0;
		let mut nodes_to_remove = DoublyLinkedList::<FreeListEntry>::new();
		for node in self.list.iter() {
			if i >= MINIMUM_POOL_ENTRIES {
				self.list.remove(node.clone());
				nodes_to_remove.push(node);
			}

			i += 1;
		}

		// Loop through all nodes in the nodes_to_remove list.
		let mut nodes_to_remove_iter = nodes_to_remove.iter();
		loop {
			// Before deallocating memory for any node, ensure that the minimum number of entries is in the node pool.
			let mut i = 0;
			for _node in self.list.iter() {
				i += 1;
				if i == MINIMUM_POOL_ENTRIES {
					break;
				}
			}

			for _j in 0..(MINIMUM_POOL_ENTRIES-i) {
				let entry = Node::new(
					FreeListEntry {
						start: 0,
						end: 0
					}
				);
				self.list.push(entry);
			}

			// Now check if there is a node to remove.
			if let Some(node) = nodes_to_remove_iter.next() {
				// There is, so let Rust deallocate its memory by removing it from the list.
				// This calls our deallocation routine again, which itself tries another Pool Maintenance.
				// But as we set maintenance_in_progress to true, no infinite recursion takes place.
				nodes_to_remove.remove(node);
			} else {
				// There are no more nodes to remove and the pool contains at least the minimum number of entries.
				break;
			}
		}

		self.maintenance_in_progress = false;
	}

	#[allow(dead_code)]
	pub fn print_information(&self) {
		let mut i = 0;
		for _node in self.list.iter() {
			i += 1;
		}

		infoheader!(" POOL INFORMATION ");
		info!("{} elements in Node Pool", i);
		infofooter!();
	}
}
