use std::sync::Arc;
use std::path::PathBuf;

use app_lib::domain::traits::{MockMediaStorer, Storer};

pub fn get_media_store() -> Storer {
    let mut mock_store = MockMediaStorer::new();

    mock_store.expect_add_file().returning(|path, _suggested_series| Ok(PathBuf::from(path)));

    /*mock_store
        .expect_as_local_path()
        .returning(|collection, video| format!("/{}/{}", collection, video));*/

    Arc::new(mock_store)
}
