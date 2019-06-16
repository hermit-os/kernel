// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Implementation of a Doubly Linked List in Safe Rust using reference-counting.
//!
//! In contrast to collections provided by Rust, this implementation can insert
//! and remove entries at a specific position in O(1) given an iterator.

use alloc::rc::Rc;
use core::cell::RefCell;

pub struct DoublyLinkedList<T> {
	head: Option<Rc<RefCell<Node<T>>>>,
	tail: Option<Rc<RefCell<Node<T>>>>,
}

pub struct Node<T> {
	pub value: T,
	prev: Option<Rc<RefCell<Node<T>>>>,
	next: Option<Rc<RefCell<Node<T>>>>,
}

impl<T> Node<T> {
	pub fn new(value: T) -> Rc<RefCell<Self>> {
		Rc::new(RefCell::new(Self { value: value, prev: None, next: None }))
	}
}

impl<T> DoublyLinkedList<T> {
	pub const fn new() -> Self {
		Self { head: None, tail: None }
	}

	pub fn head(&self) -> Option<Rc<RefCell<Node<T>>>> {
		self.head.as_ref().map(|node| node.clone())
	}

	pub fn tail(&self) -> Option<Rc<RefCell<Node<T>>>> {
		self.tail.as_ref().map(|node| node.clone())
	}

	pub fn push(&mut self, new_node: Rc<RefCell<Node<T>>>) {
		{
			let mut new_node_borrowed = new_node.borrow_mut();

			// We expect a node that is currently not mounted to any list.
			assert!(new_node_borrowed.prev.is_none() && new_node_borrowed.next.is_none());

			// Check if we already have any nodes in the list.
			match self.tail.take() {
				Some(tail) => {
					// We become the next node of the old list tail and the old list tail becomes our previous node.
					tail.borrow_mut().next = Some(new_node.clone());
					new_node_borrowed.prev = Some(tail);
				},
				None => {
					// No nodes yet, so we become the new list head.
					self.head = Some(new_node.clone());
				}
			}
		}

		// In any case, we become the new list tail.
		self.tail = Some(new_node);
	}

	pub fn insert_before(&mut self, new_node: Rc<RefCell<Node<T>>>, node: Rc<RefCell<Node<T>>>) {
		let mut node_borrowed = node.borrow_mut();

		{
			let mut new_node_borrowed = new_node.borrow_mut();

			// We expect a node that is currently not mounted to any list.
			assert!(new_node_borrowed.prev.is_none() && new_node_borrowed.next.is_none());

			// Check if the given node is the first one in the list.
			match node_borrowed.prev.take() {
				Some(prev_node) => {
					// It is not, so its previous node now becomes our previous node.
					prev_node.borrow_mut().next = Some(new_node.clone());
					new_node_borrowed.prev = Some(prev_node);
				},
				None => {
					// It is, so we become the new list head.
					self.head = Some(new_node.clone());
				}
			}

			// The given node becomes our next node.
			new_node_borrowed.next = Some(node.clone());
		}

		// We become the previous node of the given node.
		node_borrowed.prev = Some(new_node);
	}

	pub fn insert_after(&mut self, new_node: Rc<RefCell<Node<T>>>, node: Rc<RefCell<Node<T>>>) {
		let mut node_borrowed = node.borrow_mut();

		{
			let mut new_node_borrowed = new_node.borrow_mut();

			// We expect a node that is currently not mounted to any list.
			assert!(new_node_borrowed.prev.is_none() && new_node_borrowed.next.is_none());

			// Check if the given node is the last one in the list.
			match node_borrowed.next.take() {
				Some(next_node) => {
					// It is not, so its next node now becomes our next node.
					next_node.borrow_mut().prev = Some(new_node.clone());
					new_node_borrowed.next = Some(next_node);
				},
				None => {
					// It is, so we become the new list tail.
					self.tail = Some(new_node.clone());
				}
			}

			// The given node becomes our previous node.
			new_node_borrowed.prev = Some(node.clone());
		}

		// We become the next node of the given node.
		node_borrowed.next = Some(new_node);
	}

	pub fn remove(&mut self, node: Rc<RefCell<Node<T>>>) {
		// Unmount the previous and next nodes of the node to remove.
		let (prev, next) = {
			let mut borrowed = node.borrow_mut();
			(borrowed.prev.take(), borrowed.next.take())
		};

		// Clone the next node, so we can still check it after remounting.
		let next_clone = next.clone();

		// Check the previous node.
		// If we have one, remount the next node to that previous one, skipping our node to remove.
		// If not, the next node becomes the new list head.
		match prev {
			Some(ref prev_node) => prev_node.borrow_mut().next = next,
			None => self.head = next
		};

		// Check the cloned next node.
		// If we have one, remount the previous node to that next one, skipping our node to remove.
		// If not, the previous node becomes the new list tail.
		match next_clone {
			Some(ref next_node) => next_node.borrow_mut().prev = prev,
			None => self.tail = prev
		};
	}

	pub fn iter(&self) -> Iter<T> {
		Iter::<T> { current: self.head.as_ref().map(|node| node.clone()) }
	}
}

impl<T> Default for DoublyLinkedList<T> {
	fn default() -> Self {
		Self { head: None, tail: None }
	}
}

pub struct Iter<T> {
	current: Option<Rc<RefCell<Node<T>>>>
}

impl<T> Iterator for Iter<T> {
	type Item = Rc<RefCell<Node<T>>>;

	fn next(&mut self) -> Option<Self::Item> {
		// If we have a current node, replace it by a clone of the next node.
		// Then we can still return the (previously) current one.
		self.current.take().map(|node| {
			self.current = node.borrow().next.clone();
			node
		})
	}
}
