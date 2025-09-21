use axum::http::StatusCode;
use std::sync::Arc;

use app_lib::domain::traits::{ MockRemotePlayer, RemotePlayer};


pub fn get_remote_player() -> Arc<dyn RemotePlayer> {
    let mut player = MockRemotePlayer::new();
    player
        .expect_send()
        .times(2)
        .returning(|_| Ok(StatusCode::OK));

    Arc::new(player)
}
