#![deny(missing_docs, missing_debug_implementations)]
#![cfg_attr(test, deny(warnings))]
#![doc(html_root_url = "https://docs.rs/scoped-mut-tls/0.1.0")]

//! Scoped, mutable, thread-local storage
//!
//! This module provides the ability to generate *scoped* thread-local variables. In this sense,
//! scoped indicates that thread local storage actually stores a mutable reference to a value, and
//! this mutable reference is only placed in storage for a scoped amount of time.
//!
//! There are no restrictions on what types can be placed into a scoped variable, but all scoped
//! variables are initialized to the equivalent of null. Scoped thread local storage is useful when
//! a value is present for a known period of time and it is not required to relinquish ownership of
//! the contents.
//!
//! # Examples
//!
//! ```
//! #[macro_use]
//! extern crate scoped_mut_tls;
//!
//! scoped_mut_thread_local!(static FOO: u32);
//!
//! # fn main() {
//! // Initially each scoped slot is empty.
//! assert!(!FOO.is_set());
//!
//! let mut num = 1;
//!
//! // When inserting a value, the value is only in place for the duration
//! // of the closure specified.
//! FOO.set(&mut num, || {
//!     FOO.with(|slot| {
//!         assert_eq!(*slot, 1);
//!         *slot = 2;
//!     });
//! });
//!
//! assert_eq!(num, 2);
//! # }
//! ```

#[macro_export]
macro_rules! scoped_mut_thread_local {
    (static $name:ident: $ty:ty) => (
        static $name: $crate::ScopedMutKey<$ty> = $crate::ScopedMutKey {
            inner: {
                thread_local!(static FOO: ::std::cell::Cell<usize> = {
                    ::std::cell::Cell::new(0)
                });
                &FOO
            },
            _marker: ::std::marker::PhantomData,
        };
    )
}

use std::cell::Cell;
use std::fmt;
use std::marker;
use std::thread::LocalKey;

/// Type representing a thread local storage key corresponding to a mutable reference to the type
/// parameter `T`.
///
/// Keys are statically allocated and can contain a reference to an instance of type `T` scoped to
/// a particular lifetime. Keys provides two methods, `set` and `with`, both of which currently use
/// closures to control the scope of their contents.
pub struct ScopedMutKey<T> {
    #[doc(hidden)]
    pub inner: &'static LocalKey<Cell<usize>>,
    #[doc(hidden)]
    pub _marker: marker::PhantomData<T>,
}

unsafe impl<T> Sync for ScopedMutKey<T> {}

struct Reset<'a> {
    cell: &'a Cell<usize>,
    val: usize,
}

impl<'a> Drop for Reset<'a> {
    fn drop(&mut self) {
        self.cell.set(self.val);
    }
}

impl<T> ScopedMutKey<T> {
    /// Inserts a value into this scoped thread local storage slot for a duration of a closure.
    ///
    /// While `cb` is running, the value `t` will be returned by `get` unless this function is
    /// called recursively inside of `cb`.
    ///
    /// Upon return, this function will restore the previous value, if any was available.
    ///
    /// # Examples
    ///
    /// ```
    /// #[macro_use]
    /// extern crate scoped_mut_tls;
    ///
    /// scoped_mut_thread_local!(static FOO: u32);
    ///
    /// # fn main() {
    /// let mut num = 100;
    ///
    /// FOO.set(&mut num, || {
    ///     let val = FOO.with(|v| *v);
    ///     assert_eq!(val, 100);
    ///
    ///     // set can be called recursively
    ///     FOO.set(&mut 101, || {
    ///         // ...
    ///     });
    ///
    ///     // Recursive calls restore the previous value.
    ///     let val = FOO.with(|v| *v);
    ///     assert_eq!(val, 100);
    ///
    ///     // The referenced value can be mutated
    ///     FOO.with(|v| *v = 200);
    /// });
    ///
    /// assert_eq!(num, 200);
    /// # }
    /// ```
    pub fn set<F, R>(&'static self, t: &mut T, f: F) -> R
        where F: FnOnce() -> R
    {
        self.inner.with(|cell| {
            let prev = cell.get();
            cell.set(t as *mut _ as usize);

            let _reset = Reset {
                cell: cell,
                val: prev,
            };

            f()
        })
    }

    /// Gets a value out of this scoped variable.
    ///
    /// This function takes a closure which receives the value of this variable.
    ///
    /// # Panics
    ///
    /// This function will panic if `set` has not previously been called.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// #[macro_use]
    /// extern crate scoped_mut_tls;
    ///
    /// scoped_mut_thread_local!(static FOO: u32);
    ///
    /// # fn main() {
    /// FOO.with(|slot| {
    ///     // work with `slot`
    /// # drop(slot);
    /// });
    /// # }
    /// ```
    pub fn with<F, R>(&'static self, f: F) -> R
        where F: FnOnce(&mut T) -> R
    {
        self.inner.with(|cell| {
            let val = cell.get();
            cell.set(0);

            assert!(val != 0, "cannot access a scoped thread local \
                               variable without calling `set` first");

            let _reset = Reset {
                cell: cell,
                val,
            };

            unsafe {
                f(&mut *(val as *mut T))
            }
        })
    }

    /// Test whether this TLS key has been `set` for the current thread.
    pub fn is_set(&'static self) -> bool {
        self.inner.with(|c| c.get() != 0)
    }
}

impl<T: fmt::Debug> fmt::Debug for ScopedMutKey<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("ScopedMutKey")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::mpsc::{channel, Sender};
    use std::thread;

    scoped_mut_thread_local!(static FOO: u32);

    #[test]
    fn smoke() {
        scoped_mut_thread_local!(static BAR: u32);

        assert!(!BAR.is_set());
        BAR.set(&mut 1, || {
            assert!(BAR.is_set());
            BAR.with(|slot| {
                assert_eq!(*slot, 1);
            });
        });
        assert!(!BAR.is_set());
    }

    #[test]
    fn cell_allowed() {
        scoped_mut_thread_local!(static BAR: Cell<u32>);

        BAR.set(&mut Cell::new(1), || {
            BAR.with(|slot| {
                assert_eq!(slot.get(), 1);
            });
        });
    }

    #[test]
    fn scope_item_allowed() {
        assert!(!FOO.is_set());
        FOO.set(&mut 1, || {
            assert!(FOO.is_set());
            FOO.with(|slot| {
                assert_eq!(*slot, 1);
            });
        });
        assert!(!FOO.is_set());
    }

    #[test]
    fn panic_resets() {
        struct Check(Sender<u32>);
        impl Drop for Check {
            fn drop(&mut self) {
                FOO.with(|r| {
                    self.0.send(*r).unwrap();
                })
            }
        }

        let (tx, rx) = channel();
        let t = thread::spawn(|| {
            FOO.set(&mut 1, || {
                let _r = Check(tx);

                FOO.set(&mut 2, || {
                    panic!()
                });
            });
        });

        assert_eq!(rx.recv().unwrap(), 1);
        assert!(t.join().is_err());
    }
}
