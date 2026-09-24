//! One SoA column: a contiguous, aligned array of fixed-size rows (§3
//! "stored column-wise"). Rows are raw bytes laid out by the component
//! schema; a Rust component views the same bytes as `&[T]` with no copy.
//! Cloning a column is one `memcpy`, which is what makes snapshots cheap (§2, §6).
//!
//! Borrowing is checked at runtime per column (like `RefCell`), so a query
//! that asks for `&mut T` and `&T` of the same component in one pass gets a
//! structured error instead of aliasing.

use crate::error::{codes, ArgusError, Result};
use std::alloc::{self, Layout};
use std::cell::Cell;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

/// A column of `len` rows, each `stride` bytes, aligned to `align`.
pub struct Column {
    ptr: NonNull<u8>,
    cap: usize,
    len: usize,
    stride: usize,
    align: usize,
    /// >0 shared borrows, -1 exclusive borrow, 0 free.
    borrow: Cell<isize>,
}

impl std::fmt::Debug for Column {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Column")
            .field("len", &self.len)
            .field("stride", &self.stride)
            .field("align", &self.align)
            .finish()
    }
}

impl Column {
    /// Empty column. `stride == 0` is a tag column that never allocates.
    pub fn new(stride: usize, align: usize) -> Column {
        Column {
            ptr: NonNull::<u8>::dangling(),
            cap: 0,
            len: 0,
            stride,
            align: align.max(1),
            borrow: Cell::new(0),
        }
    }

    /// Row count.
    pub fn len(&self) -> usize {
        self.len
    }
    /// True if no rows.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Bytes per row.
    pub fn stride(&self) -> usize {
        self.stride
    }
    /// Bytes currently allocated.
    pub fn capacity_bytes(&self) -> usize {
        self.cap * self.stride
    }

    fn layout(&self, cap: usize) -> Layout {
        Layout::from_size_align(cap * self.stride, self.align).expect("column layout")
    }

    fn grow_to(&mut self, min_cap: usize) {
        if self.stride == 0 || min_cap <= self.cap {
            return;
        }
        let new_cap = min_cap.max(self.cap * 2).max(16);
        let new_layout = self.layout(new_cap);
        // SAFETY: layout has non-zero size (stride > 0, new_cap >= 16); when
        // cap > 0 the old pointer came from the same allocator with the old layout.
        let ptr = unsafe {
            if self.cap == 0 {
                alloc::alloc(new_layout)
            } else {
                alloc::realloc(self.ptr.as_ptr(), self.layout(self.cap), new_layout.size())
            }
        };
        self.ptr = NonNull::new(ptr).unwrap_or_else(|| alloc::handle_alloc_error(new_layout));
        self.cap = new_cap;
    }

    /// Reserve room for `extra` more rows.
    pub fn reserve(&mut self, extra: usize) {
        self.grow_to(self.len + extra);
    }

    /// Raw pointer to a row. Caller checks `row < len`.
    #[inline]
    pub fn row_ptr(&self, row: usize) -> *const u8 {
        debug_assert!(row < self.len);
        // SAFETY: row < len <= cap.
        unsafe { self.ptr.as_ptr().add(row * self.stride) }
    }

    /// Mutable raw pointer to a row. Caller checks `row < len` and the
    /// borrow state.
    #[inline]
    pub fn row_ptr_mut(&self, row: usize) -> *mut u8 {
        debug_assert!(row < self.len);
        // SAFETY: row < len <= cap.
        unsafe { self.ptr.as_ptr().add(row * self.stride) }
    }

    /// Append a row by copying `stride` bytes from `src`. Returns the new row.
    ///
    /// # Safety
    /// `src` must be valid for reads of `stride` bytes.
    pub unsafe fn push_from(&mut self, src: *const u8) -> usize {
        self.grow_to(self.len + 1);
        let row = self.len;
        if self.stride > 0 {
            core::ptr::copy_nonoverlapping(
                src,
                self.ptr.as_ptr().add(row * self.stride),
                self.stride,
            );
        }
        self.len += 1;
        row
    }

    /// Every row's bytes, in row order.
    pub fn bytes(&self) -> &[u8] {
        if self.stride == 0 || self.len == 0 {
            return &[];
        }
        // SAFETY: the first len * stride bytes are initialised rows.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len * self.stride) }
    }

    /// Append a row from a byte slice of exactly `stride` bytes.
    pub fn push(&mut self, bytes: &[u8]) -> usize {
        assert_eq!(bytes.len(), self.stride, "row size mismatch");
        // SAFETY: slice length checked.
        unsafe { self.push_from(bytes.as_ptr()) }
    }

    /// Remove `row` by moving the last row into its place. Returns true if a
    /// row was moved (i.e. the removed row was not the last).
    pub fn swap_remove(&mut self, row: usize) -> bool {
        assert!(row < self.len, "swap_remove out of range");
        let last = self.len - 1;
        let moved = row != last;
        if moved && self.stride > 0 {
            // SAFETY: both rows in bounds and distinct.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.ptr.as_ptr().add(last * self.stride),
                    self.ptr.as_ptr().add(row * self.stride),
                    self.stride,
                );
            }
        }
        self.len = last;
        moved
    }

    /// Drop all rows (keeps capacity).
    pub fn clear(&mut self) {
        self.len = 0;
    }

    fn check_typed<T>(&self) {
        assert_eq!(
            core::mem::size_of::<T>(),
            self.stride,
            "typed view: size mismatch"
        );
        assert!(
            core::mem::align_of::<T>() <= self.align,
            "typed view: alignment mismatch"
        );
    }

    /// Borrow the whole column as `&[T]`. Fails if exclusively borrowed.
    pub fn borrow<T>(&self) -> Result<ColRef<'_, T>> {
        self.check_typed::<T>();
        let b = self.borrow.get();
        if b < 0 {
            return Err(borrow_conflict("mutably"));
        }
        self.borrow.set(b + 1);
        // SAFETY: size/align checked; rows are initialized; shared borrow recorded.
        let slice = unsafe { core::slice::from_raw_parts(self.ptr.as_ptr() as *const T, self.len) };
        Ok(ColRef { col: self, slice })
    }

    /// Borrow the whole column as `&mut [T]`. Fails if borrowed at all.
    pub fn borrow_mut<T>(&self) -> Result<ColMut<'_, T>> {
        self.check_typed::<T>();
        if self.borrow.get() != 0 {
            return Err(borrow_conflict("already"));
        }
        self.borrow.set(-1);
        // SAFETY: size/align checked; exclusive borrow recorded; data is behind
        // a raw pointer so mutation through `&self` is sound.
        let slice =
            unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr() as *mut T, self.len) };
        Ok(ColMut { col: self, slice })
    }

    /// Try to take a dynamic shared borrow (for field-level access).
    pub(crate) fn acquire_shared(&self) -> Result<()> {
        let b = self.borrow.get();
        if b < 0 {
            return Err(borrow_conflict("mutably"));
        }
        self.borrow.set(b + 1);
        Ok(())
    }
    /// Try to take a dynamic exclusive borrow.
    pub(crate) fn acquire_exclusive(&self) -> Result<()> {
        if self.borrow.get() != 0 {
            return Err(borrow_conflict("already"));
        }
        self.borrow.set(-1);
        Ok(())
    }
    pub(crate) fn release_shared(&self) {
        self.borrow.set(self.borrow.get() - 1);
    }
    pub(crate) fn release_exclusive(&self) {
        self.borrow.set(0);
    }
}

fn borrow_conflict(how: &str) -> ArgusError {
    ArgusError::new(codes::BORROW_CONFLICT, format!("column is {how} borrowed"))
        .hint("A query may not fetch the same component twice in one pass; fetch it once as &mut.")
}

impl Clone for Column {
    fn clone(&self) -> Column {
        let mut c = Column::new(self.stride, self.align);
        if self.stride > 0 && self.len > 0 {
            c.grow_to(self.len);
            // SAFETY: c has capacity for len rows; self has len initialized rows.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.ptr.as_ptr(),
                    c.ptr.as_ptr(),
                    self.len * self.stride,
                );
            }
        }
        c.len = self.len;
        c
    }
}

impl Drop for Column {
    fn drop(&mut self) {
        if self.cap > 0 && self.stride > 0 {
            // SAFETY: allocated with this layout in grow_to.
            unsafe { alloc::dealloc(self.ptr.as_ptr(), self.layout(self.cap)) }
        }
    }
}

/// Shared typed view of a column. Releases the borrow on drop.
pub struct ColRef<'a, T> {
    col: &'a Column,
    slice: &'a [T],
}

impl<T> Deref for ColRef<'_, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.slice
    }
}

impl<T> Drop for ColRef<'_, T> {
    fn drop(&mut self) {
        self.col.release_shared();
    }
}

/// Exclusive typed view of a column. Releases the borrow on drop.
pub struct ColMut<'a, T> {
    col: &'a Column,
    slice: &'a mut [T],
}

impl<T> Deref for ColMut<'_, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.slice
    }
}

impl<T> DerefMut for ColMut<'_, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.slice
    }
}

impl<T> Drop for ColMut<'_, T> {
    fn drop(&mut self) {
        self.col.release_exclusive();
    }
}
