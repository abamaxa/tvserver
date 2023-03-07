use std::sync::Arc;

use tvserver::domain::traits::{MediaStorer, MockMediaStorer};

pub fn get_media_store() -> Arc<dyn MediaStorer> {
    let mut mock_store = MockMediaStorer::new();

    mock_store.expect_move_file().returning(|_| Ok(()));

    Arc::new(mock_store)
}
