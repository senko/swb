//! [`ByteBudget`] and [`Reservation`]: a byte limit over the memory that
//! the requests of a script's API calls (`fetch()`) hold outside the
//! script heap (M8 feature 10a, ADR 0028 section 16).
//!
//! A document has one budget. Each API request takes a [`Reservation`] from
//! it before the request is queued: the size of the request (URL, headers,
//! body). The worker that reads the response grows the reservation with
//! every chunk it reads, so a response that would pass the limit stops
//! before the memory is allocated ([`NetError::OverBudget`](crate::NetError)).
//! The reservation travels with the request, then with the
//! [`Completion`](crate::Completion), and is dropped when the bytes reach
//! the script heap or are dropped (abort, navigation, stop). A worker that
//! cannot be interrupted keeps the request body alive; its reservation
//! stays until the worker lets the request go.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A limit over a sum of bytes, shared by the reservations that draw on it.
#[derive(Debug)]
pub struct ByteBudget {
    limit: usize,
    used: AtomicUsize,
}

impl ByteBudget {
    /// Creates a budget of `limit` bytes.
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(ByteBudget {
            limit,
            used: AtomicUsize::new(0),
        })
    }

    /// The bytes that reservations hold now.
    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    /// The limit in bytes.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Takes `bytes` if the sum stays within the limit.
    fn try_add(&self, bytes: usize) -> bool {
        self.used
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes).filter(|sum| *sum <= self.limit)
            })
            .is_ok()
    }

    fn sub(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// The bytes of one API request: its own size and the response body that is
/// read or held. Returns its bytes to the budget when dropped.
#[derive(Debug)]
pub struct Reservation {
    budget: Arc<ByteBudget>,
    request: AtomicUsize,
    response: AtomicUsize,
}

/// Two reservations are equal only if they are the same object (a
/// [`Request`](crate::Request) is compared by value otherwise).
impl PartialEq for Reservation {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for Reservation {}

impl Reservation {
    /// Takes `request_bytes` from `budget`, or returns `None` if they do not
    /// fit.
    pub fn new(budget: &Arc<ByteBudget>, request_bytes: usize) -> Option<Arc<Reservation>> {
        budget.try_add(request_bytes).then(|| {
            Arc::new(Reservation {
                budget: Arc::clone(budget),
                request: AtomicUsize::new(request_bytes),
                response: AtomicUsize::new(0),
            })
        })
    }

    /// Sets the bytes that the response holds to `bytes`. Returns false (and
    /// keeps the old value) if the budget cannot grow by the difference.
    /// Only the worker that runs the request calls it.
    pub fn set_response(&self, bytes: usize) -> bool {
        let old = self.response.load(Ordering::Relaxed);
        if bytes > old {
            if !self.budget.try_add(bytes - old) {
                return false;
            }
        } else {
            self.budget.sub(old - bytes);
        }
        self.response.store(bytes, Ordering::Relaxed);
        true
    }

    /// Returns the bytes of the request itself (the worker dropped it).
    pub fn release_request(&self) {
        let bytes = self.request.swap(0, Ordering::Relaxed);
        self.budget.sub(bytes);
    }

    /// The bytes that this reservation holds.
    pub fn bytes(&self) -> usize {
        self.request.load(Ordering::Relaxed) + self.response.load(Ordering::Relaxed)
    }

    /// The limit of the budget.
    pub fn limit(&self) -> usize {
        self.budget.limit
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.sub(self.bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_stay_within_the_limit_and_return_their_bytes() {
        let budget = ByteBudget::new(100);
        let a = Reservation::new(&budget, 60).unwrap();
        assert!(Reservation::new(&budget, 41).is_none());
        let b = Reservation::new(&budget, 40).unwrap();
        assert_eq!(budget.used(), 100);
        assert!(!a.set_response(1));
        drop(b);
        assert!(a.set_response(40));
        assert!(!a.set_response(41));
        assert_eq!(budget.used(), 100);
        a.release_request();
        assert_eq!(budget.used(), 40);
        assert!(a.set_response(10));
        assert_eq!(budget.used(), 10);
        drop(a);
        assert_eq!(budget.used(), 0);
    }
}
