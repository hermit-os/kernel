use alloc::vec::Vec;

/// Allocator for new [`ObjectPool`] items.
pub(crate) trait ObjectAllocator<T> {
	/// Allocates a new object.
	fn allocate(&self) -> T;
}

/// A generic object pool that is manually managed.
///
/// Objects are retrieved via [`ObjectPool::get`] and need to be manually
/// returned via [`ObjectPool::put`]. Not returning an object is not
/// problematic, but should usually be done to avoid re-allocating.
pub(crate) struct ObjectPool<T, A: ObjectAllocator<T>> {
	/// Underlying allocator that is used as a fallback source of items.
	allocator: A,
	/// Cache of items that were already allocated in the past and returned.
	cache: Vec<T>,
}

impl<T, A> ObjectPool<T, A>
where
	A: ObjectAllocator<T>,
{
	/// Creates a new object pool with the given backing allocator.
	pub fn new(allocator: A) -> Self {
		Self {
			allocator,
			cache: Vec::new(),
		}
	}

	/// Retrieve an object from the pool. This will take items from the cache
	/// if any are available, otherwise it will allocate a new item.
	pub fn get(&mut self) -> T {
		self.cache
			.pop()
			.unwrap_or_else(|| self.allocator.allocate())
	}

	/// Returns an item back to the object pool.
	pub(crate) fn put(&mut self, item: T) {
		self.cache.push(item);
	}
}
