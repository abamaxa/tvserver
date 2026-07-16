use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

#[derive(Clone, Copy, Eq, PartialEq)]
enum LeaseState {
    Processing,
    Reconciling,
}

#[derive(Clone, Default)]
pub struct BookPathLeaseCoordinator {
    inner: Arc<BookPathLeaseState>,
}

#[derive(Default)]
struct BookPathLeaseState {
    states: Mutex<HashMap<PathBuf, LeaseState>>,
    changed: Notify,
}

impl BookPathLeaseCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn acquire_processing(&self, path: &Path) -> BookPathLease {
        loop {
            let changed = self.inner.changed.notified();
            let acquired = {
                let mut states = self
                    .inner
                    .states
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if states.contains_key(path) {
                    false
                } else {
                    states.insert(path.to_path_buf(), LeaseState::Processing);
                    true
                }
            };
            if acquired {
                return BookPathLease {
                    inner: self.inner.clone(),
                    path: path.to_path_buf(),
                    state: LeaseState::Processing,
                };
            }
            changed.await;
        }
    }

    pub fn try_acquire_reconciling(&self, path: &Path) -> Option<BookPathLease> {
        let mut states = self
            .inner
            .states
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if states.contains_key(path) {
            return None;
        }
        states.insert(path.to_path_buf(), LeaseState::Reconciling);
        Some(BookPathLease {
            inner: self.inner.clone(),
            path: path.to_path_buf(),
            state: LeaseState::Reconciling,
        })
    }
}

pub struct BookPathLease {
    inner: Arc<BookPathLeaseState>,
    path: PathBuf,
    state: LeaseState,
}

impl Drop for BookPathLease {
    fn drop(&mut self) {
        {
            let mut states = self
                .inner
                .states
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if states.get(&self.path) == Some(&self.state) {
                states.remove(&self.path);
            }
        }
        self.inner.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::BookPathLeaseCoordinator;
    use tokio::{sync::oneshot, time::Duration};

    #[tokio::test]
    async fn processing_lease_makes_reconciliation_unavailable() {
        let coordinator = BookPathLeaseCoordinator::new();
        let path = Path::new("library/collection/book.epub");

        let processing = coordinator.acquire_processing(path).await;

        assert!(coordinator.try_acquire_reconciling(path).is_none());
        drop(processing);
        assert!(coordinator.try_acquire_reconciling(path).is_some());
    }

    #[tokio::test]
    async fn reconciling_lease_makes_processing_wait_until_release() {
        let coordinator = BookPathLeaseCoordinator::new();
        let path = Path::new("library/collection/book.epub");
        let reconciling = coordinator.try_acquire_reconciling(path).unwrap();
        let waiting_coordinator = coordinator.clone();
        let waiting_path = path.to_path_buf();
        let (acquired_tx, mut acquired_rx) = oneshot::channel();
        let waiter = tokio::spawn(async move {
            let processing = waiting_coordinator.acquire_processing(&waiting_path).await;
            acquired_tx.send(()).unwrap();
            processing
        });

        assert!(tokio::time::timeout(Duration::from_millis(25), &mut acquired_rx)
            .await
            .is_err());
        drop(reconciling);
        tokio::time::timeout(Duration::from_secs(1), &mut acquired_rx)
            .await
            .expect("processing should acquire after reconciliation releases")
            .unwrap();
        drop(waiter.await.unwrap());
    }
}
