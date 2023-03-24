use std::sync::Arc;

use tvserver::domain::traits::{MockMediaStorer, Storer};

pub fn get_media_store() -> Storer {
    let mut mock_store = MockMediaStorer::new();

    mock_store.expect_move_file().returning(|_| Ok(()));

    mock_store
        .expect_as_path()
        .returning(|collection, video| format!("/{}/{}", collection, video));

    Arc::new(mock_store)
}
